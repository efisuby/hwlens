// SPDX-License-Identifier: Apache-2.0

#include <fstream>
#include <random>

#include <gtest/gtest.h>

#include "hwlens_history.hpp"

namespace fs = std::filesystem;
using History::json;

namespace {
	struct TempDir {
		fs::path path;
		TempDir() {
			std::random_device rd;
			path = fs::temp_directory_path() / ("hwlens-history-" + std::to_string(rd()));
			fs::create_directories(path);
		}
		~TempDir() {
			std::error_code ec;
			fs::remove_all(path, ec);
		}
	};

	auto tick(int64_t t, double cpu, std::vector<History::ProcStat> procs = {}) -> History::Tick {
		return {t, {{"cpu", cpu}, {"mem", 50.0}}, std::move(procs)};
	}

	//? One sample per minute starting at <start>, cpu = minute index
	void fill(History::Store& s, int64_t start, int minutes) {
		for (int m = 0; m <= minutes; m++) s.add(tick(start + m * 60, m));
	}
}

TEST(history, minute_average_peak_and_top) {
	History::Store s({}, 7);
	s.add(tick(600, 10, {{1, "a", 50, 100, 0}, {2, "b", 10, 900, 4000}}));
	s.add(tick(630, 30, {{1, "a", 30, 100, 0}, {2, "b", 10, 800, 0}}));
	EXPECT_TRUE(s.samples().empty()) << "the minute is not over";
	s.add(tick(660, 99));
	ASSERT_EQ(s.samples().size(), 1u);
	const auto& x = s.samples().front();
	EXPECT_EQ(x.t, 600);
	EXPECT_DOUBLE_EQ(x.values.at("cpu"), 20.0);
	EXPECT_DOUBLE_EQ(x.values.at("cpu_max"), 30.0);
	EXPECT_DOUBLE_EQ(x.values.at("mem"), 50.0);
	EXPECT_EQ(x.top["cpu"][0][1], "a");
	EXPECT_DOUBLE_EQ(x.top["cpu"][0][2].get<double>(), 40.0);
	EXPECT_EQ(x.top["mem"][0][1], "b");
	EXPECT_DOUBLE_EQ(x.top["mem"][0][2].get<double>(), 900.0) << "memory is the peak of the minute";
	EXPECT_EQ(x.top["io"].size(), 1u) << "processes without I/O are not ranked";
	EXPECT_DOUBLE_EQ(x.top["io"][0][2].get<double>(), 2000.0);
}

TEST(history, survives_restart_and_drops_old) {
	TempDir dir;
	const auto file = dir.path / "history.jsonl";
	const int64_t day = 86400;
	{
		History::Store s(file, 1);
		fill(s, 10 * day, 3);
		ASSERT_EQ(s.samples().size(), 3u);
	}
	{
		History::Store s(file, 1);
		s.load(10 * day + 600);
		ASSERT_EQ(s.samples().size(), 3u);
		EXPECT_DOUBLE_EQ(s.samples()[2].values.at("cpu"), 2.0);
	}
	{
		History::Store s(file, 1);
		s.load(11 * day + 30); // the last day only: 10 days + 30 s and later
		ASSERT_EQ(s.samples().size(), 2u);
		EXPECT_EQ(s.samples().front().t, 10 * day + 60);
	}
	std::ifstream f(file);
	size_t lines = 0;
	for (std::string l; std::getline(f, l);) lines++;
	EXPECT_EQ(lines, 2u) << "the file was rewritten without the old samples";
}

TEST(history, load_skips_broken_lines) {
	TempDir dir;
	const auto file = dir.path / "history.jsonl";
	std::ofstream(file) << R"({"t":60,"v":{"cpu":1}})" << "\n{broken\n" << R"({"t":120,"v":{"cpu":2}})" << "\n";
	History::Store s(file, 7);
	s.load(200);
	EXPECT_EQ(s.samples().size(), 2u);
}

TEST(history, query_downsamples_and_keeps_peaks) {
	History::Store s({}, 7);
	fill(s, 0, 10); // samples at 0, 60, ..., 540 with cpu 0..9
	const auto r = s.query({{"from", 0}, {"to", 599}, {"points", 5}}, 600);
	ASSERT_TRUE(r["ok"]);
	EXPECT_EQ(r["step"], 120);
	ASSERT_EQ(r["t"].size(), 5u);
	EXPECT_EQ(r["t"][1], 120);
	EXPECT_DOUBLE_EQ(r["series"]["cpu"][1].get<double>(), 2.5);
	EXPECT_DOUBLE_EQ(r["series"]["cpu_max"][1].get<double>(), 3.0);
	EXPECT_EQ(r["oldest"], 0);
	EXPECT_EQ(r["newest"], 540);
}

TEST(history, query_at_returns_nearest) {
	History::Store s({}, 7);
	fill(s, 0, 5);
	EXPECT_EQ(s.query({{"at", 100}}, 400)["sample"]["t"], 120);
	EXPECT_EQ(s.query({{"at", 80}}, 400)["sample"]["t"], 60);
	EXPECT_EQ(s.query({{"at", 9999}}, 400)["sample"]["t"], 240);
}

TEST(history, query_rejects_bad_bodies) {
	History::Store s({}, 7);
	EXPECT_FALSE(s.query(json::array(), 0)["ok"]);
	EXPECT_FALSE(s.query({{"from", "x"}}, 0)["ok"]);
	EXPECT_FALSE(s.query({{"from", 10}, {"to", 5}}, 0)["ok"]);
}

TEST(history, disabled_keeps_nothing) {
	History::Store s({}, 0);
	fill(s, 0, 3);
	EXPECT_TRUE(s.samples().empty());
}
