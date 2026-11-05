/* Copyright 2026 Alexander Baranovskiy

   Licensed under the Apache License, Version 2.0 (the "License");
   you may not use this file except in compliance with the License.
   You may obtain a copy of the License at

	   http://www.apache.org/licenses/LICENSE-2.0

   Unless required by applicable law or agreed to in writing, software
   distributed under the License is distributed on an "AS IS" BASIS,
   WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
   See the License for the specific language governing permissions and
   limitations under the License.
*/

#pragma once

#include <vector>

#include <nlohmann/json.hpp>

#include "hwlens_shared.hpp"

//* Snapshot of the collected state as JSON, shared by hwlensd and its clients.
//*
//* A snapshot holds everything the UI reads: the collect() results and the collector globals
//* (cpu name, sensors, selected net interface, ...). hwlensd sends one full snapshot after
//* "hello" and then a delta per tick, see diff() and patch().
namespace Json {
	using json = nlohmann::json;

	//* Bumped on incompatible changes of the snapshot layout or the protocol
	constexpr int schema_version = 1;

	//* Results of one collection cycle
	struct State {
		Cpu::cpu_info cpu;
		Mem::mem_info mem;
		std::vector<Proc::proc_info> procs;
		uint64_t total_mem{};
	#ifdef GPU_SUPPORT
		std::vector<Gpu::gpu_info> gpus;
	#endif
	};

	//* Serialize to one line; invalid UTF-8 (process names and command lines can hold any bytes) is replaced
	auto dump(const json& j) -> std::string;

	//* Build a snapshot from <state> and the collector globals
	auto snapshot(const State& state) -> json;

	//* Fill <state> and the collector globals from <snap>. Throws json::exception on bad input.
	void restore(const json& snap, State& state);

	//* Changes that turn <prev> into <cur>, or a discarded value when they are equal.
	//* Number arrays (graph histories) that only got new samples are sent as their new tail.
	auto diff(const json& prev, const json& cur) -> json;

	//* Apply a diff() result to <target>
	void patch(json& target, const json& delta);
}
