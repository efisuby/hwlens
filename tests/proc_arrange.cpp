// SPDX-License-Identifier: Apache-2.0

#include <string>
#include <vector>

#include <gtest/gtest.h>

#include "hwlens_config.hpp"
#include "hwlens_shared.hpp"

namespace {
	using Proc::proc_info;

	//? 1 init ─┬─ 10 bash ── 11 vim
	//?         └─ 20 sshd ── 21 pipewire
	//? 30 orphan, its parent 999 is gone
	auto sample() -> std::vector<proc_info> {
		auto make = [](size_t pid, uint64_t ppid, const std::string& name, double cpu) {
			proc_info p{};
			p.pid = pid;
			p.ppid = ppid;
			p.name = name;
			p.cmd = "/usr/bin/" + name;
			p.user = "user";
			p.cpu_p = cpu;
			p.state = 'S';
			return p;
		};
		return {make(21, 20, "pipewire", 4), make(1, 0, "init", 1), make(11, 10, "vim", 3),
				make(30, 999, "orphan", 0), make(10, 1, "bash", 2), make(20, 1, "sshd", 5)};
	}

	void setup(bool tree, const std::string& filter = "", bool aggregate = false) {
		Config::set("proc_sorting", std::string{"pid"});
		Config::set("proc_reversed", true); //? "pid" sorts descending, reversed gives ascending
		Config::set("proc_tree", tree);
		Config::set("proc_filter", filter);
		Config::set("proc_aggregate", aggregate);
		Config::set("pause_proc_list", false);
	}

	auto visible_pids(std::vector<proc_info> procs) -> std::vector<size_t> {
		std::erase_if(procs, [](const proc_info& p) { return p.filtered; });
		std::vector<size_t> pids;
		for (const auto& p : procs) pids.push_back(p.pid);
		return pids;
	}

	auto find(std::vector<proc_info>& procs, size_t pid) -> proc_info& {
		return *std::ranges::find(procs, pid, &proc_info::pid);
	}
}

TEST(proc_arrange, list_sorted) {
	setup(false);
	auto procs = sample();
	Proc::arrange(procs, true);
	EXPECT_EQ(visible_pids(procs), (std::vector<size_t>{1, 10, 11, 20, 21, 30}));
	EXPECT_EQ(Proc::numpids, 6);
}

TEST(proc_arrange, list_filtered) {
	setup(false, "vim");
	auto procs = sample();
	Proc::arrange(procs, true);
	EXPECT_EQ(visible_pids(procs), (std::vector<size_t>{11}));
	EXPECT_EQ(Proc::numpids, 1);
}

TEST(proc_arrange, tree_order_and_depth) {
	setup(true);
	auto procs = sample();
	Proc::arrange(procs, true);
	EXPECT_EQ(visible_pids(procs), (std::vector<size_t>{1, 10, 11, 20, 21, 30}));
	EXPECT_EQ(find(procs, 11).depth, 2u);
	EXPECT_EQ(find(procs, 21).depth, 2u);
	//? The orphan is attached to the root
	EXPECT_EQ(find(procs, 30).ppid, 0u);
	EXPECT_EQ(find(procs, 30).depth, 0u);
}

TEST(proc_arrange, tree_filtered_keeps_match_only) {
	setup(true, "pipewire");
	auto procs = sample();
	Proc::arrange(procs, true);
	EXPECT_EQ(visible_pids(procs), (std::vector<size_t>{21}));
	EXPECT_EQ(Proc::numpids, 1);
}

TEST(proc_arrange, aggregate_only_on_fresh_data) {
	setup(true, "", true);
	auto procs = sample();
	Proc::arrange(procs, true);
	//? bash 2% + vim 3%
	EXPECT_DOUBLE_EQ(find(procs, 10).cpu_p, 5.0);
	//? Re-ordering the same data must not add the children again
	Proc::arrange(procs, false);
	EXPECT_DOUBLE_EQ(find(procs, 10).cpu_p, 5.0);
}
