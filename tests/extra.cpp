// SPDX-License-Identifier: Apache-2.0

#include <fstream>
#include <random>

#include <gtest/gtest.h>

#include "hwlens_extra.hpp"

namespace fs = std::filesystem;

namespace {
	//? A temporary directory tree, removed at the end of the test
	struct Tree {
		fs::path root;

		Tree() {
			std::random_device rd;
			root = fs::temp_directory_path() / ("hwlens-extra-" + std::to_string(rd()));
			fs::create_directories(root);
		}
		~Tree() {
			std::error_code ec;
			fs::remove_all(root, ec);
		}
		void write(const std::string& rel, const std::string& content) const {
			const auto p = root / rel;
			fs::create_directories(p.parent_path());
			std::ofstream(p) << content;
		}
		void link(const std::string& rel, const std::string& target) const {
			const auto p = root / rel;
			fs::create_directories(p.parent_path());
			fs::create_symlink(target, p);
		}
	};

	auto find(const std::vector<Extra::Sensor>& v, const std::string& label) -> const Extra::Sensor* {
		for (const auto& s : v) if (s.label == label) return &s;
		return nullptr;
	}
}

TEST(extra, decode_ipv4_endpoint) {
	EXPECT_EQ(Extra::decode_endpoint("0100007F:0035"), "127.0.0.1:53");
	EXPECT_EQ(Extra::decode_endpoint("00000000:0016"), "0.0.0.0:22");
	EXPECT_EQ(Extra::decode_endpoint("5C01A8C0:C350"), "192.168.1.92:50000");
}

TEST(extra, decode_ipv6_endpoint) {
	EXPECT_EQ(Extra::decode_endpoint("00000000000000000000000001000000:0277"), "[::1]:631");
	EXPECT_EQ(Extra::decode_endpoint("00000000000000000000000000000000:0016"), "[::]:22");
	EXPECT_EQ(Extra::decode_endpoint("000080FE00000000FF0F00000100A8C0:1F90"), "[fe80::fff:c0a8:1]:8080");
}

TEST(extra, decode_rejects_garbage) {
	EXPECT_EQ(Extra::decode_endpoint("nonsense"), "");
	EXPECT_EQ(Extra::decode_endpoint("0100:0035"), "");
}

TEST(extra, hwmon_units_labels_and_limits) {
	Tree t;
	t.write("class/hwmon/hwmon0/name", "coretemp\n");
	t.write("class/hwmon/hwmon0/temp1_input", "45000\n");
	t.write("class/hwmon/hwmon0/temp1_label", "Package id 0\n");
	t.write("class/hwmon/hwmon0/temp1_max", "80000\n");
	t.write("class/hwmon/hwmon0/temp1_crit", "100000\n");
	t.write("class/hwmon/hwmon0/temp2_input", "-273000\n"); // disconnected
	t.write("class/hwmon/hwmon1/name", "nct6798\n");
	t.write("class/hwmon/hwmon1/fan2_input", "796\n");
	t.write("class/hwmon/hwmon1/pwm2", "255\n");
	t.write("class/hwmon/hwmon1/in0_input", "1200\n");
	t.write("class/hwmon/hwmon1/in0_label", "Vcore\n");
	t.write("class/hwmon/hwmon1/power1_average", "19500000\n");
	t.write("class/hwmon/hwmon1/curr1_input", "2500\n");
	t.write("class/hwmon/hwmon1/temp3_input", "not a number\n");

	const auto s = Extra::read_hwmon(t.root);
	ASSERT_EQ(s.size(), 6u);

	const auto* pkg = find(s, "Package id 0");
	ASSERT_NE(pkg, nullptr);
	EXPECT_EQ(pkg->chip, "coretemp");
	EXPECT_EQ(pkg->kind, "temp");
	EXPECT_DOUBLE_EQ(pkg->value, 45.0);
	EXPECT_DOUBLE_EQ(pkg->max.value_or(0), 80.0);
	EXPECT_DOUBLE_EQ(pkg->crit.value_or(0), 100.0);

	EXPECT_DOUBLE_EQ(find(s, "fan2")->value, 796.0);
	EXPECT_DOUBLE_EQ(find(s, "pwm2")->value, 100.0);
	EXPECT_DOUBLE_EQ(find(s, "Vcore")->value, 1.2);
	EXPECT_EQ(find(s, "Vcore")->kind, "voltage");
	EXPECT_DOUBLE_EQ(find(s, "power1")->value, 19.5);
	EXPECT_DOUBLE_EQ(find(s, "curr1")->value, 2.5);

	//? Sorted by chip, then kind: coretemp temp first, nct fan before pwm before power
	EXPECT_EQ(s.front().chip, "coretemp");
	EXPECT_EQ(s[1].kind, "fan");
	EXPECT_EQ(s[2].kind, "pwm");
}

TEST(extra, hwmon_prefers_input_and_names_duplicate_chips) {
	Tree t;
	t.write("class/hwmon/hwmon2/name", "nvme\n");
	t.write("class/hwmon/hwmon2/temp1_input", "40000\n");
	t.write("class/hwmon/hwmon10/name", "nvme\n");
	t.write("class/hwmon/hwmon10/temp1_input", "50000\n");
	t.write("class/hwmon/hwmon3/device/name", "legacy\n");
	t.write("class/hwmon/hwmon3/device/power1_input", "1000000\n");
	t.write("class/hwmon/hwmon3/device/power1_average", "2000000\n");

	const auto s = Extra::read_hwmon(t.root);
	ASSERT_EQ(s.size(), 3u);
	std::vector<std::string> chips;
	for (const auto& x : s) chips.push_back(x.chip);
	EXPECT_NE(std::ranges::find(chips, "nvme"), chips.end());
	EXPECT_NE(std::ranges::find(chips, "nvme #2"), chips.end());
	for (const auto& x : s) {
		if (x.chip == "nvme") {
			EXPECT_DOUBLE_EQ(x.value, 40.0) << "hwmon2 comes before hwmon10";
		}
		if (x.chip == "legacy") {
			EXPECT_DOUBLE_EQ(x.value, 1.0) << "power1_input wins over power1_average";
		}
	}
}

TEST(extra, stat_and_system_rates) {
	Tree t;
	t.write("stat", "cpu  1 2 3 4\nintr 1000 5 6\nctxt 5000\nbtime 1\nprocesses 300\nprocs_running 3\nprocs_blocked 1\n");
	t.write("sys/kernel/hostname", "box\n");
	t.write("sys/kernel/osrelease", "7.0.0-42-generic\n");
	t.write("uptime", "3600.50 100.00\n");
	t.write("sys/fs/file-nr", "12000\t0\t9223372036854775807\n");
	t.write("etc/os-release", "NAME=\"Ubuntu\"\nPRETTY_NAME=\"Ubuntu 26.04 LTS\"\n");

	const auto prev = Extra::read_stat(t.root);
	ASSERT_TRUE(prev);
	EXPECT_EQ(prev->ctxt, 5000u);
	EXPECT_EQ(prev->intr, 1000u);
	EXPECT_EQ(prev->forks, 300u);

	auto cur = *prev;
	cur.ctxt += 1000;
	cur.intr += 500;
	cur.forks += 5;
	const auto j = Extra::system_info(t.root, t.root / "etc", prev, cur, 2.0);
	EXPECT_EQ(j["hostname"], "box");
	EXPECT_EQ(j["kernel"], "7.0.0-42-generic");
	EXPECT_EQ(j["os"], "Ubuntu 26.04 LTS");
	EXPECT_EQ(j["uptime"], 3600);
	EXPECT_EQ(j["open_files"], 12000);
	EXPECT_EQ(j["ctxt_per_s"], 500.0);
	EXPECT_EQ(j["intr_per_s"], 250.0);
	EXPECT_EQ(j["forks_per_s"], 2.5);
	EXPECT_EQ(j["procs_running"], 3);
}

TEST(extra, proc_io_rates_skip_idle_and_new) {
	Tree t;
	t.write("10/io", "rchar: 1\nwchar: 2\nread_bytes: 1000\nwrite_bytes: 0\n");
	t.write("11/io", "read_bytes: 0\nwrite_bytes: 0\n");
	Extra::Collector c;
	c.proc = t.root;
	c.sys = t.root / "nosys";
	c.etc = t.root / "noetc";
	c.collect({10, 11}, 0.0);

	t.write("10/io", "read_bytes: 5000\nwrite_bytes: 2000\n");
	t.write("12/io", "read_bytes: 9999\nwrite_bytes: 9999\n"); // new: no rate yet
	const auto j = c.collect({10, 11, 12}, 2.0);
	const auto& io = j["proc_io"];
	ASSERT_TRUE(io.contains("10"));
	EXPECT_EQ(io["10"][0], 2000.0);
	EXPECT_EQ(io["10"][1], 1000.0);
	EXPECT_FALSE(io.contains("11")) << "idle process listed";
	EXPECT_FALSE(io.contains("12")) << "process without a previous reading listed";
}

TEST(extra, sockets_with_owners) {
	Tree t;
	t.write("net/tcp",
		"  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n"
		"   0: 00000000:0016 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 111 1 0\n"
		"   1: 0100007F:1F90 0100007F:C350 01 00000000:00000000 00:00000000 00000000  1000        0 222 1 0\n"
		"   2: 0100007F:1F90 0100007F:C351 06 00000000:00000000 00:00000000 00000000  1000        0 0 1 0\n");
	t.write("net/udp",
		"  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode ref pointer drops\n"
		"   5: 00000000:0035 00000000:0000 07 00000000:00000000 00:00000000 00000000     0        0 333 2 0 0\n");
	t.link("42/fd/3", "socket:[222]");
	t.link("42/fd/4", "/dev/null");
	t.link("7/fd/9", "socket:[111]");

	const auto owners = Extra::socket_owners(t.root);
	EXPECT_EQ(owners.at(222), 42);
	EXPECT_EQ(owners.at(111), 7);
	EXPECT_FALSE(owners.contains(333));

	Extra::Collector c;
	c.proc = t.root;
	c.sys = t.root / "nosys";
	const auto j = c.collect({}, 1.0)["sockets"];
	EXPECT_EQ(j["total"], 4);
	EXPECT_EQ(j["udp"], 1);
	EXPECT_EQ(j["tcp_states"]["LISTEN"], 1);
	EXPECT_EQ(j["tcp_states"]["TIME_WAIT"], 1);
	const auto& list = j["list"];
	ASSERT_EQ(list.size(), 3u) << "time-wait is not listed";
	EXPECT_EQ(list[0]["state"], "LISTEN");
	EXPECT_EQ(list[0]["pid"], 7);
	EXPECT_EQ(list[0]["local"], "0.0.0.0:22");
	EXPECT_EQ(list[1]["state"], "UNCONN");
	EXPECT_EQ(list[2]["state"], "ESTABLISHED");
	EXPECT_EQ(list[2]["pid"], 42);
	EXPECT_EQ(list[2]["remote"], "127.0.0.1:50000");
}
