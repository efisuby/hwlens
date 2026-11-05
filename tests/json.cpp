// SPDX-License-Identifier: Apache-2.0

#include <gtest/gtest.h>

#include "hwlens_json.hpp"

using Json::json;

namespace {
	//? patch(prev, diff(prev, cur)) must give cur back
	void expect_roundtrip(const json& prev, const json& cur) {
		const auto delta = Json::diff(prev, cur);
		if (prev == cur) {
			EXPECT_TRUE(delta.is_discarded());
			return;
		}
		json target = prev;
		Json::patch(target, delta);
		EXPECT_EQ(target, cur) << "delta: " << delta.dump();
	}

	auto history(long long first, size_t n) -> json {
		json arr = json::array();
		for (size_t i = 0; i < n; i++) arr.push_back(first + static_cast<long long>(i));
		return arr;
	}
}

TEST(json_diff, equal_is_discarded) {
	const json j = {{"a", 1}, {"b", {1, 2, 3}}};
	EXPECT_TRUE(Json::diff(j, j).is_discarded());
}

TEST(json_diff, history_sends_tail) {
	//? Growing history
	const json prev = {{"h", history(0, 5)}};
	const json cur = {{"h", history(0, 6)}};
	const auto delta = Json::diff(prev, cur);
	EXPECT_EQ(delta.dump().find("[0,1,2"), std::string::npos) << "full history sent: " << delta.dump();
	expect_roundtrip(prev, cur);
	//? History at its size limit: one sample in, one out
	expect_roundtrip({{"h", history(0, 600)}}, {{"h", history(1, 600)}});
	//? Two samples between deltas
	expect_roundtrip({{"h", history(0, 600)}}, {{"h", history(2, 600)}});
	//? Constant history (ambiguous overlap) and a reset to a shorter one
	expect_roundtrip({{"h", json(std::vector<int>(10, 7))}}, {{"h", json(std::vector<int>(10, 7))}});
	expect_roundtrip({{"h", history(0, 10)}}, {{"h", history(100, 3)}});
	expect_roundtrip({{"h", history(0, 10)}}, {{"h", json::array()}});
}

TEST(json_diff, objects_add_change_remove) {
	const json prev = {{"keep", 1}, {"change", "a"}, {"gone", true}, {"nested", {{"x", 1}, {"y", 2}}}};
	const json cur = {{"keep", 1}, {"change", "b"}, {"new", nullptr}, {"nested", {{"x", 1}, {"y", 3}}}};
	expect_roundtrip(prev, cur);
	//? Keys that look like markers are plain data
	expect_roundtrip({{"$del", 1}}, {{"$del", 2}, {"tail", {1, 2}}});
}

TEST(json_diff, lists_of_containers) {
	//? Per core histories
	json prev = json::array({history(0, 40), history(10, 40), history(20, 40)});
	json cur = json::array({history(1, 40), history(10, 40), history(21, 40)});
	expect_roundtrip(prev, cur);
	//? Length change falls back to the full list
	expect_roundtrip(prev, json::array({history(0, 40)}));
	//? Type changes
	expect_roundtrip({{"a", {1, 2}}}, {{"a", {{"x", 1}}}});
	expect_roundtrip({{"a", {{"x", 1}}}}, {{"a", 5}});
}

TEST(json_snapshot, restore_roundtrip) {
	Json::State state;
	state.cpu.cpu_percent.at("total") = {1, 2, 3};
	state.cpu.core_percent = {{5, 6}, {7, 8}};
	state.cpu.temp = {{40, 41}, {42}};
	state.cpu.load_avg = {0.5, 1.5, 2.5};
	state.cpu.active_cpus = std::vector<std::int32_t>{0, 1};
	state.cpu.fans = {{"fan2", 900, 35}};
	Mem::disk_info disk;
	disk.name = "root";
	disk.dev = "/dev/nvme0n1p2";
	disk.io_read = {1, 2};
	state.mem.disks["/"] = disk;
	state.mem.disks_order = {"/"};
	Proc::proc_info p;
	p.pid = 42;
	p.ppid = 1;
	p.name = "bash";
	p.cmd = "/bin/bash -l";
	p.state = 'S';
	p.cpu_p = 1.25;
	state.procs = {p};
	Cpu::cpuName = "Test CPU";
	Cpu::core_kinds = {'P', 'E'};
	Cpu::current_bat = {80, 5.5f, 3600, "Discharging"};
	Net::current_net["eth0"].bandwidth.at("download") = {10, 20};
	Net::interfaces = {"eth0"};
	Net::selected_iface = "eth0";
	Proc::gpu_mem = {{42, 1024}};

	const auto snap = Json::snapshot(state);
	Json::State restored;
	Cpu::cpuName.clear();
	Cpu::core_kinds.clear();
	Net::current_net.clear();
	Proc::gpu_mem.clear();
	Json::restore(snap, restored);

	EXPECT_EQ(Cpu::cpuName, "Test CPU");
	EXPECT_EQ(Cpu::core_kinds, (std::vector<char>{'P', 'E'}));
	EXPECT_EQ(std::get<3>(Cpu::current_bat), "Discharging");
	EXPECT_EQ(restored.cpu.core_percent.size(), 2u);
	EXPECT_EQ(restored.mem.disks.at("/").dev, "/dev/nvme0n1p2");
	ASSERT_EQ(restored.procs.size(), 1u);
	EXPECT_EQ(restored.procs[0].name, "bash");
	EXPECT_EQ(restored.procs[0].state, 'S');
	EXPECT_DOUBLE_EQ(restored.procs[0].cpu_p, 1.25);
	EXPECT_EQ(Net::current_net.at("eth0").bandwidth.at("download").back(), 20);
	EXPECT_EQ(Proc::gpu_mem.at(42), 1024u);
	//? Snapshot of the restored state is the same
	EXPECT_EQ(Json::snapshot(restored), snap);
}

TEST(json_snapshot, rejects_other_versions) {
	Json::State state;
	auto snap = Json::snapshot(state);
	snap["v"] = Json::schema_version + 1;
	EXPECT_THROW(Json::restore(snap, state), std::runtime_error);
}
