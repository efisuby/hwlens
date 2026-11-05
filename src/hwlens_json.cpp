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

#include "hwlens_json.hpp"

#include <algorithm>
#include <string>

//* Serializers, found by argument dependent lookup, so they live in the namespace of the type
namespace Cpu {
	NLOHMANN_DEFINE_TYPE_NON_INTRUSIVE_WITH_DEFAULT(fan_info, label, rpm, pwm_percent)
}

namespace Mem {
	NLOHMANN_DEFINE_TYPE_NON_INTRUSIVE_WITH_DEFAULT(disk_info, dev, name, fstype, total, used, free, used_percent,
		free_percent, temp_crit, temp, io_read, io_write, io_activity)
	NLOHMANN_DEFINE_TYPE_NON_INTRUSIVE_WITH_DEFAULT(mem_info, stats, percent, disks, disks_order)
}

namespace Net {
	NLOHMANN_DEFINE_TYPE_NON_INTRUSIVE_WITH_DEFAULT(net_stat, speed, top, total, last, offset, rollover)
	NLOHMANN_DEFINE_TYPE_NON_INTRUSIVE_WITH_DEFAULT(net_info, bandwidth, stat, ipv4, ipv6, connected)
}

#ifdef GPU_SUPPORT
namespace Gpu {
	NLOHMANN_DEFINE_TYPE_NON_INTRUSIVE_WITH_DEFAULT(gpu_info_supported, gpu_utilization, mem_utilization, gpu_clock,
		mem_clock, pwr_usage, pwr_state, temp_info, mem_total, mem_used, pcie_txrx, encoder_utilization,
		decoder_utilization, fan_speed, clocks_event_reasons)
	NLOHMANN_DEFINE_TYPE_NON_INTRUSIVE_WITH_DEFAULT(gpu_info, gpu_percent, gpu_clock_speed, pwr_usage, pwr_max_usage,
		pwr_state, temp, temp_max, mem_total, mem_used, mem_utilization_percent, mem_clock_speed, pcie_tx, pcie_rx,
		encoder_utilization, decoder_utilization, fan_speed, clocks_event_reasons, supported_functions)
}
#endif

namespace Json {
	namespace {
		//? Markers of diff() operations. They start with a NUL byte, which no interface, mount point
		//? or other data key can contain.
		const string op_del{"\0del", 4};
		const string op_tail{"\0tail", 5};
		const string op_size{"\0size", 5};
		const string op_at{"\0at", 3};

		//? Doubles read from a snapshot; JSON has no NaN or inf, nlohmann writes them as null
		auto num(const json& j, const char* key, double fallback = 0.0) -> double {
			const auto it = j.find(key);
			return (it != j.end() and it->is_number()) ? it->get<double>() : fallback;
		}

		auto cpu_to_json(const Cpu::cpu_info& c) -> json {
			json j = {
				{"cpu_percent", c.cpu_percent}, {"core_percent", c.core_percent}, {"temp", c.temp},
				{"temp_max", c.temp_max}, {"load_avg", c.load_avg}, {"pressure", c.pressure},
				{"usage_watts", c.usage_watts}, {"fans", c.fans},
			};
			j["active_cpus"] = c.active_cpus.has_value() ? json(*c.active_cpus) : json(nullptr);
			return j;
		}

		void cpu_from_json(const json& j, Cpu::cpu_info& c) {
			j.at("cpu_percent").get_to(c.cpu_percent);
			j.at("core_percent").get_to(c.core_percent);
			j.at("temp").get_to(c.temp);
			c.temp_max = j.value("temp_max", 0ll);
			for (size_t i = 0; i < 3; i++) {
				c.load_avg[i] = j.at("load_avg").at(i).is_number() ? j.at("load_avg").at(i).get<double>() : 0.0;
				c.pressure[i] = j.at("pressure").at(i).is_number() ? j.at("pressure").at(i).get<double>() : -1.0;
			}
			c.usage_watts = static_cast<float>(num(j, "usage_watts"));
			j.at("fans").get_to(c.fans);
			const auto& active = j.at("active_cpus");
			c.active_cpus = active.is_null() ? std::nullopt : std::optional{active.get<std::vector<std::int32_t>>()};
		}

		auto proc_to_json(const Proc::proc_info& p) -> json {
			//? UI only fields (prefix, depth, tree_index, collapsed, filtered) are left out
			return {
				{"pid", p.pid}, {"name", p.name}, {"cmd", p.cmd}, {"threads", p.threads}, {"user", p.user},
				{"mem", p.mem}, {"cpu_p", p.cpu_p}, {"cpu_c", p.cpu_c}, {"state", string(1, p.state)},
				{"nice", p.p_nice}, {"ppid", p.ppid}, {"cpu_s", p.cpu_s}, {"cpu_t", p.cpu_t},
				{"death_time", p.death_time}, {"unit", p.unit},
			};
		}

		void proc_from_json(const json& j, Proc::proc_info& p) {
			p.pid = j.at("pid").get<size_t>();
			p.name = j.value("name", "");
			p.cmd = j.value("cmd", "");
			p.threads = j.value("threads", size_t{});
			p.user = j.value("user", "");
			p.mem = j.value("mem", uint64_t{});
			p.cpu_p = num(j, "cpu_p");
			p.cpu_c = num(j, "cpu_c");
			const auto state = j.value("state", "0");
			p.state = state.empty() ? '0' : state.front();
			p.p_nice = j.value("nice", int64_t{});
			p.ppid = j.value("ppid", uint64_t{});
			p.cpu_s = j.value("cpu_s", uint64_t{});
			p.cpu_t = j.value("cpu_t", uint64_t{});
			p.death_time = j.value("death_time", uint64_t{});
			p.unit = j.value("unit", "");
		}

		auto detail_to_json(const Proc::detail_container& d) -> json {
			return {
				{"last_pid", d.last_pid}, {"skip_smaps", d.skip_smaps}, {"entry", proc_to_json(d.entry)},
				{"elapsed", d.elapsed}, {"parent", d.parent}, {"status", d.status}, {"io_read", d.io_read},
				{"io_write", d.io_write}, {"memory", d.memory}, {"first_mem", d.first_mem},
				{"cpu_percent", d.cpu_percent}, {"mem_bytes", d.mem_bytes},
			};
		}

		void detail_from_json(const json& j, Proc::detail_container& d) {
			d.last_pid = j.value("last_pid", size_t{});
			d.skip_smaps = j.value("skip_smaps", false);
			proc_from_json(j.at("entry"), d.entry);
			d.elapsed = j.value("elapsed", "");
			d.parent = j.value("parent", "");
			d.status = j.value("status", "");
			d.io_read = j.value("io_read", "");
			d.io_write = j.value("io_write", "");
			d.memory = j.value("memory", "");
			d.first_mem = j.value("first_mem", -1ll);
			j.at("cpu_percent").get_to(d.cpu_percent);
			j.at("mem_bytes").get_to(d.mem_bytes);
		}

		auto all_numbers(const json& arr) -> bool {
			return std::ranges::all_of(arr, [](const json& v) { return v.is_number(); });
		}

		auto all_containers(const json& arr) -> bool {
			return std::ranges::all_of(arr, [](const json& v) { return v.is_array() or v.is_object(); });
		}
	}

	auto dump(const json& j) -> string {
		return j.dump(-1, ' ', false, json::error_handler_t::replace);
	}

	auto snapshot(const State& state) -> json {
		json snap;
		snap["v"] = schema_version;
		snap["shared"] = {{"cores", Shared::coreCount}, {"total_mem", state.total_mem}};

		auto& cpu = snap["cpu"];
		cpu["info"] = cpu_to_json(state.cpu);
		cpu["name"] = Cpu::cpuName;
		cpu["hz"] = Cpu::cpuHz;
		cpu["fields"] = Cpu::available_fields;
		cpu["sensors"] = Cpu::available_sensors;
		cpu["got_sensors"] = Cpu::got_sensors;
		cpu["temp_only"] = Cpu::cpu_temp_only;
		cpu["has_battery"] = Cpu::has_battery;
		cpu["has_fans"] = Cpu::has_fans;
		cpu["watts"] = Cpu::supports_watts;
		cpu["kinds"] = string(Cpu::core_kinds.begin(), Cpu::core_kinds.end());
		cpu["batteries"] = Cpu::available_batteries;
		cpu["battery"] = Cpu::current_bat;
		cpu["core_map"] = Cpu::core_mapping;
		cpu["container"] = Cpu::container_engine.has_value() ? json(*Cpu::container_engine) : json(nullptr);

		snap["mem"] = {
			{"info", state.mem}, {"has_swap", Mem::has_swap}, {"disk_ios", Mem::disk_ios}, {"redraw", Mem::redraw},
		};

		snap["net"] = {
			{"ifaces", Net::current_net}, {"interfaces", Net::interfaces}, {"selected", Net::selected_iface},
			{"graph_max", Net::graph_max}, {"redraw", Net::redraw},
		};

		//? Processes keyed by pid, so a delta carries only the fields that changed per process
		json list = json::object();
		for (const auto& p : state.procs) list[std::to_string(p.pid)] = proc_to_json(p);
		json gpu_mem = json::object();
		for (const auto& [pid, bytes] : Proc::gpu_mem) gpu_mem[std::to_string(pid)] = bytes;
		snap["proc"] = {
			{"list", std::move(list)}, {"detail", detail_to_json(Proc::detailed)}, {"gpu_mem", std::move(gpu_mem)},
			{"gpu_mem_supported", Proc::gpu_mem_supported}, {"redraw", Proc::redraw},
		};

	#ifdef GPU_SUPPORT
		snap["gpu"] = {
			{"list", state.gpus}, {"names", Gpu::gpu_names}, {"supported", Gpu::gpu_supported},
			{"count", Gpu::count}, {"pwr_total_max", Gpu::gpu_pwr_total_max},
			{"shared_percent", Gpu::shared_gpu_percent},
		};
	#endif
		return snap;
	}

	void restore(const json& snap, State& state) {
		if (snap.value("v", 0) != schema_version)
			throw std::runtime_error("unsupported snapshot version " + std::to_string(snap.value("v", 0)));

		const auto& shared = snap.at("shared");
		Shared::coreCount = shared.value("cores", 1l);
		state.total_mem = shared.value("total_mem", uint64_t{});

		const auto& cpu = snap.at("cpu");
		cpu_from_json(cpu.at("info"), state.cpu);
		Cpu::cpuName = cpu.value("name", "");
		Cpu::cpuHz = cpu.value("hz", "");
		cpu.at("fields").get_to(Cpu::available_fields);
		cpu.at("sensors").get_to(Cpu::available_sensors);
		Cpu::got_sensors = cpu.value("got_sensors", false);
		Cpu::cpu_temp_only = cpu.value("temp_only", false);
		Cpu::has_battery = cpu.value("has_battery", false);
		Cpu::has_fans = cpu.value("has_fans", false);
		Cpu::supports_watts = cpu.value("watts", false);
		const auto kinds = cpu.value("kinds", "");
		Cpu::core_kinds.assign(kinds.begin(), kinds.end());
		cpu.at("batteries").get_to(Cpu::available_batteries);
		const auto& bat = cpu.at("battery");
		Cpu::current_bat = {bat.at(0).get<int>(), bat.at(1).is_number() ? bat.at(1).get<float>() : 0.0f,
							bat.at(2).get<long>(), bat.at(3).get<string>()};
		cpu.at("core_map").get_to(Cpu::core_mapping);
		const auto& container = cpu.at("container");
		Cpu::container_engine = container.is_null() ? std::nullopt : std::optional{container.get<string>()};

		const auto& mem = snap.at("mem");
		mem.at("info").get_to(state.mem);
		Mem::has_swap = mem.value("has_swap", false);
		Mem::disk_ios = mem.value("disk_ios", 0);
		if (mem.value("redraw", false)) Mem::redraw = true;

		const auto& net = snap.at("net");
		net.at("ifaces").get_to(Net::current_net);
		net.at("interfaces").get_to(Net::interfaces);
		Net::selected_iface = net.value("selected", "");
		net.at("graph_max").get_to(Net::graph_max);
		if (net.value("redraw", false)) Net::redraw = true;

		const auto& proc = snap.at("proc");
		state.procs.clear();
		state.procs.reserve(proc.at("list").size());
		for (const auto& [pid, p] : proc.at("list").items()) {
			proc_from_json(p, state.procs.emplace_back());
		}
		detail_from_json(proc.at("detail"), Proc::detailed);
		Proc::gpu_mem.clear();
		for (const auto& [pid, bytes] : proc.at("gpu_mem").items()) Proc::gpu_mem[std::stoull(pid)] = bytes.get<uint64_t>();
		Proc::gpu_mem_supported = proc.value("gpu_mem_supported", false);
		if (proc.value("redraw", false)) Proc::redraw = true;

	#ifdef GPU_SUPPORT
		if (const auto it = snap.find("gpu"); it != snap.end()) {
			const auto& gpu = *it;
			gpu.at("list").get_to(state.gpus);
			gpu.at("names").get_to(Gpu::gpu_names);
			gpu.at("supported").get_to(Gpu::gpu_supported);
			Gpu::count = gpu.value("count", 0);
			Gpu::gpu_pwr_total_max = gpu.value("pwr_total_max", 0ll);
			gpu.at("shared_percent").get_to(Gpu::shared_gpu_percent);
		}
	#endif
	}

	auto diff(const json& prev, const json& cur) -> json {
		if (prev == cur) return json(json::value_t::discarded);

		if (prev.is_object() and cur.is_object()) {
			json out = json::object();
			for (const auto& [key, value] : cur.items()) {
				const auto it = prev.find(key);
				if (it == prev.end()) {
					out[key] = value;
				}
				else if (auto d = diff(*it, value); not d.is_discarded()) {
					out[key] = std::move(d);
				}
			}
			for (const auto& [key, value] : prev.items()) {
				if (not cur.contains(key)) out[key] = {{op_del, 1}};
			}
			return out;
		}

		if (prev.is_array() and cur.is_array() and not cur.empty()) {
			//? A history that got new samples: the new tail and the resulting size
			if (all_numbers(prev) and all_numbers(cur)) {
				const size_t n = cur.size();
				for (size_t k = 1; k < n; k++) {
					const size_t keep = n - k;
					if (prev.size() < keep) continue;
					if (std::equal(prev.end() - keep, prev.end(), cur.begin())) {
						return {{op_tail, json(cur.begin() + keep, cur.end())}, {op_size, n}};
					}
				}
			}
			//? Same length list of containers (per core histories, GPUs): changed elements only
			else if (prev.size() == cur.size() and all_containers(prev) and all_containers(cur)) {
				json at = json::object();
				for (size_t i = 0; i < cur.size(); i++) {
					if (auto d = diff(prev[i], cur[i]); not d.is_discarded()) at[std::to_string(i)] = std::move(d);
				}
				return {{op_at, std::move(at)}};
			}
		}
		return cur;
	}

	void patch(json& target, const json& delta) {
		if (delta.is_object()) {
			if (const auto tail = delta.find(op_tail); tail != delta.end()) {
				if (not target.is_array()) throw std::runtime_error("delta tail for a non array value");
				for (const auto& v : *tail) target.push_back(v);
				const size_t n = delta.at(op_size).get<size_t>();
				if (target.size() > n) target.erase(target.begin(), target.begin() + static_cast<std::ptrdiff_t>(target.size() - n));
				return;
			}
			if (const auto at = delta.find(op_at); at != delta.end()) {
				if (not target.is_array()) throw std::runtime_error("delta index for a non array value");
				for (const auto& [index, d] : at->items()) patch(target.at(std::stoul(index)), d);
				return;
			}
			if (target.is_object()) {
				for (const auto& [key, value] : delta.items()) {
					if (value.is_object() and value.contains(op_del)) target.erase(key);
					else if (const auto it = target.find(key); it != target.end()) patch(*it, value);
					else target[key] = value;
				}
				return;
			}
		}
		target = delta;
	}
}
