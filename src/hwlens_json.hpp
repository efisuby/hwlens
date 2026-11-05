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

#include <array>
#include <string>
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

	//* Fields of Shared::CollectOptions by their config names, for the "options" request
	struct StringOption { const char* name; std::string Shared::CollectOptions::* field; };
	struct BoolOption { const char* name; bool Shared::CollectOptions::* field; };

	using Opts = Shared::CollectOptions;
	inline constexpr std::array string_options {
		StringOption{"cpu_sensor", &Opts::cpu_sensor},
		StringOption{"freq_mode", &Opts::freq_mode},
		StringOption{"cpu_core_map", &Opts::cpu_core_map},
		StringOption{"selected_battery", &Opts::selected_battery},
		StringOption{"shown_gpus", &Opts::shown_gpus},
		StringOption{"disks_filter", &Opts::disks_filter},
		StringOption{"net_iface", &Opts::net_iface},
	};
	inline constexpr std::array bool_options {
		BoolOption{"show_coretemp", &Opts::show_coretemp},
		BoolOption{"show_cpu_freq", &Opts::show_cpu_freq},
		BoolOption{"check_temp", &Opts::check_temp},
		BoolOption{"show_fans", &Opts::show_fans},
		BoolOption{"show_battery", &Opts::show_battery},
		BoolOption{"show_cpu_watts", &Opts::show_cpu_watts},
		BoolOption{"nvml_measure_pcie_speeds", &Opts::nvml_measure_pcie_speeds},
		BoolOption{"rsmi_measure_pcie_speeds", &Opts::rsmi_measure_pcie_speeds},
		BoolOption{"show_swap", &Opts::show_swap},
		BoolOption{"swap_disk", &Opts::swap_disk},
		BoolOption{"show_disks", &Opts::show_disks},
		BoolOption{"zfs_arc_cached", &Opts::zfs_arc_cached},
		BoolOption{"disk_free_priv", &Opts::disk_free_priv},
		BoolOption{"use_fstab", &Opts::use_fstab},
		BoolOption{"only_physical", &Opts::only_physical},
		BoolOption{"zfs_hide_datasets", &Opts::zfs_hide_datasets},
		BoolOption{"net_sync", &Opts::net_sync},
		BoolOption{"net_auto", &Opts::net_auto},
		BoolOption{"proc_info_smaps", &Opts::proc_info_smaps},
		BoolOption{"proc_per_core", &Opts::proc_per_core},
		BoolOption{"proc_filter_kernel", &Opts::proc_filter_kernel},
		BoolOption{"show_detailed", &Opts::show_detailed},
		BoolOption{"pause_proc_list", &Opts::pause_proc_list},
		BoolOption{"keep_dead_proc_usage", &Opts::keep_dead_proc_usage},
		BoolOption{"proc_show_unit", &Opts::proc_show_unit},
		BoolOption{"proc_show_vram", &Opts::proc_show_vram},
	};

	//* All options as an "options" request body
	auto options_json(const Shared::CollectOptions& opts) -> json;

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
