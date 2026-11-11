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

#include "../hwlens_extra.hpp"

#include <algorithm>
#include <array>
#include <charconv>
#include <chrono>
#include <cmath>
#include <cstring>
#include <fstream>
#include <limits>
#include <map>
#include <regex>
#include <sstream>

#include <arpa/inet.h>
#include <sys/utsname.h>

namespace Extra {
	namespace {
		auto read_line(const fs::path& path) -> std::optional<std::string> {
			std::ifstream f(path);
			std::string line;
			if (not f.good() or not std::getline(f, line)) return std::nullopt;
			while (not line.empty() and (line.back() == '\n' or line.back() == ' ' or line.back() == '\r')) line.pop_back();
			return line;
		}

		auto read_number(const fs::path& path) -> std::optional<double> {
			const auto line = read_line(path);
			if (not line or line->empty()) return std::nullopt;
			double v{};
			const auto* end = line->data() + line->size();
			const auto [ptr, ec] = std::from_chars(line->data(), end, v);
			if (ec != std::errc{} or ptr == line->data() or not std::isfinite(v)) return std::nullopt;
			return v;
		}

		template <typename T>
		auto parse_uint(std::string_view s, int base = 10) -> std::optional<T> {
			T v{};
			const auto [ptr, ec] = std::from_chars(s.data(), s.data() + s.size(), v, base);
			if (ec != std::errc{} or ptr != s.data() + s.size()) return std::nullopt;
			return v;
		}

		auto rate(uint64_t prev, uint64_t cur, double seconds) -> double {
			return (seconds > 0 and cur >= prev) ? static_cast<double>(cur - prev) / seconds : 0.0;
		}

		//? Kind order and the scale from sysfs units to display units
		struct Kind { const char* prefix; const char* name; double scale; };
		constexpr std::array kinds {
			Kind{"temp", "temp", 1e-3},   // m°C
			Kind{"fan", "fan", 1.0},      // rpm
			Kind{"pwm", "pwm", 100.0 / 255.0},
			Kind{"power", "power", 1e-6}, // µW
			Kind{"in", "voltage", 1e-3},  // mV
			Kind{"curr", "current", 1e-3},// mA
		};

		auto kind_rank(std::string_view kind) -> size_t {
			for (size_t i = 0; i < kinds.size(); i++) if (kind == kinds[i].name) return i;
			return kinds.size();
		}

		const std::map<std::string_view, std::string_view> tcp_states {
			{"01", "ESTABLISHED"}, {"02", "SYN_SENT"}, {"03", "SYN_RECV"}, {"04", "FIN_WAIT1"},
			{"05", "FIN_WAIT2"}, {"06", "TIME_WAIT"}, {"07", "CLOSE"}, {"08", "CLOSE_WAIT"},
			{"09", "LAST_ACK"}, {"0A", "LISTEN"}, {"0B", "CLOSING"}, {"0C", "NEW_SYN_RECV"},
		};
	}

	auto read_stat(const fs::path& proc) -> std::optional<StatCounters> {
		std::ifstream f(proc / "stat");
		if (not f.good()) return std::nullopt;
		StatCounters c;
		for (std::string key; f >> key;) {
			uint64_t* field = nullptr;
			if (key == "ctxt") field = &c.ctxt;
			else if (key == "intr") field = &c.intr;
			else if (key == "processes") field = &c.forks;
			else if (key == "procs_running") field = &c.running;
			else if (key == "procs_blocked") field = &c.blocked;
			if (field != nullptr) f >> *field;
			f.ignore(std::numeric_limits<std::streamsize>::max(), '\n');
		}
		return c;
	}

	auto system_info(const fs::path& proc, const fs::path& etc, const std::optional<StatCounters>& prev,
		const std::optional<StatCounters>& cur, double seconds) -> json {
		json out = json::object();
		out["hostname"] = read_line(proc / "sys/kernel/hostname").value_or("");
		out["kernel"] = read_line(proc / "sys/kernel/osrelease").value_or("");

		std::string os;
		if (std::ifstream f(etc / "os-release"); f.good()) {
			for (std::string line; std::getline(f, line);) {
				if (not line.starts_with("PRETTY_NAME=")) continue;
				os = line.substr(12);
				if (os.size() >= 2 and (os.front() == '"' or os.front() == '\'')) os = os.substr(1, os.size() - 2);
				break;
			}
		}
		out["os"] = os;

		utsname u{};
		out["arch"] = uname(&u) == 0 ? std::string(u.machine) : "";

		if (const auto up = read_line(proc / "uptime")) {
			double secs{};
			std::istringstream(*up) >> secs;
			out["uptime"] = static_cast<uint64_t>(secs);
			const auto now = std::chrono::duration_cast<std::chrono::seconds>(std::chrono::system_clock::now().time_since_epoch()).count();
			out["boot_time"] = static_cast<int64_t>(now - static_cast<int64_t>(secs));
		}

		if (const auto files = read_line(proc / "sys/fs/file-nr")) {
			uint64_t open{}, unused{}, max{};
			std::istringstream(*files) >> open >> unused >> max;
			out["open_files"] = open;
			out["max_files"] = max;
		}

		//? "runnable/total" in the fourth field counts every thread (task) of the system
		if (const auto load = read_line(proc / "loadavg")) {
			std::string l1, l5, l15, tasks;
			std::istringstream(*load) >> l1 >> l5 >> l15 >> tasks;
			if (const auto slash = tasks.find('/'); slash != std::string::npos) {
				try {
					out["threads"] = std::stoull(tasks.substr(slash + 1));
				}
				catch (const std::exception&) {}
			}
		}

		if (cur) {
			out["procs_running"] = cur->running;
			out["procs_blocked"] = cur->blocked;
			if (prev) {
				out["ctxt_per_s"] = std::round(rate(prev->ctxt, cur->ctxt, seconds));
				out["intr_per_s"] = std::round(rate(prev->intr, cur->intr, seconds));
				out["forks_per_s"] = std::round(rate(prev->forks, cur->forks, seconds) * 10) / 10;
			}
		}
		return out;
	}

	auto read_hwmon(const fs::path& sys) -> std::vector<Sensor> {
		static const std::regex attr{R"(^(temp|fan|pwm|power|in|curr)(\d+)(_input|_average)?$)"};
		std::vector<std::pair<std::tuple<std::string, size_t, int>, Sensor>> found;
		std::map<std::string, int> chip_seen;
		std::error_code ec;

		std::vector<fs::path> chips;
		for (const auto& e : fs::directory_iterator(sys / "class/hwmon", ec)) chips.push_back(e.path());
		std::ranges::sort(chips, [](const fs::path& a, const fs::path& b) {
			//? hwmon2 before hwmon10
			const auto num = [](const fs::path& p) { return parse_uint<int>(p.filename().string().substr(5)).value_or(0); };
			return num(a) < num(b);
		});

		for (const auto& dir : chips) {
			//? Old drivers keep the attributes in device/
			fs::path base = dir;
			if (not fs::exists(base / "name", ec) and fs::exists(base / "device/name", ec)) base = base / "device";
			std::string chip = read_line(base / "name").value_or(dir.filename().string());
			if (const int n = chip_seen[chip]++; n > 0) chip += " #" + std::to_string(n + 1);

			for (const auto& e : fs::directory_iterator(base, ec)) {
				const auto file = e.path().filename().string();
				std::smatch m;
				if (not std::regex_match(file, m, attr)) continue;
				const std::string prefix = m[1];
				const std::string index = m[2];
				const std::string suffix = m[3];
				//? pwm has no suffix; the rest need _input (or power _average)
				if ((prefix == "pwm") != suffix.empty()) continue;
				if (suffix == "_average" and prefix != "power") continue;
				if (prefix == "power" and suffix == "_average" and fs::exists(base / ("power" + index + "_input"), ec)) continue;

				const auto k = std::ranges::find_if(kinds, [&](const Kind& kd) { return prefix == kd.prefix; });
				const auto raw = read_number(e.path());
				if (not raw) continue;

				Sensor s;
				s.chip = chip;
				s.kind = k->name;
				s.value = *raw * k->scale;
				s.label = read_line(base / (prefix + index + "_label")).value_or(prefix + index);
				const auto limit = [&](const char* name) -> std::optional<double> {
					const auto v = read_number(base / (prefix + index + name));
					//? Drivers without a limit report 0
					return (v and *v > 0) ? std::optional(*v * k->scale) : std::nullopt;
				};
				s.max = limit(prefix == "power" ? "_cap" : "_max");
				s.crit = limit("_crit");
				//? Disconnected inputs: 0 rpm fans without a label still matter, absurd temperatures do not
				if (s.kind == "temp" and (s.value <= -100 or s.value >= 250)) continue;
				found.push_back({{chip, kind_rank(s.kind), parse_uint<int>(index).value_or(0)}, std::move(s)});
			}
		}
		std::ranges::sort(found, [](const auto& a, const auto& b) { return a.first < b.first; });
		std::vector<Sensor> out;
		out.reserve(found.size());
		for (auto& [_, s] : found) out.push_back(std::move(s));
		return out;
	}

	auto to_json(const std::vector<Sensor>& sensors) -> json {
		json arr = json::array();
		for (const auto& s : sensors) {
			json j = {{"chip", s.chip}, {"label", s.label}, {"kind", s.kind}, {"value", std::round(s.value * 1000) / 1000}};
			if (s.max) j["max"] = std::round(*s.max * 1000) / 1000;
			if (s.crit) j["crit"] = std::round(*s.crit * 1000) / 1000;
			arr.push_back(std::move(j));
		}
		return arr;
	}

	auto read_proc_io(const fs::path& proc, long pid) -> std::optional<IoCounters> {
		std::ifstream f(proc / std::to_string(pid) / "io");
		if (not f.good()) return std::nullopt;
		IoCounters c;
		bool any = false;
		for (std::string key; f >> key;) {
			uint64_t v{};
			if (not (f >> v)) break;
			if (key == "read_bytes:") { c.read = v; any = true; }
			else if (key == "write_bytes:") { c.write = v; any = true; }
		}
		return any ? std::optional(c) : std::nullopt;
	}

	auto decode_endpoint(std::string_view hex) -> std::string {
		const auto colon = hex.find(':');
		if (colon == std::string_view::npos) return {};
		const auto addr = hex.substr(0, colon);
		const auto port = parse_uint<uint16_t>(hex.substr(colon + 1), 16).value_or(0);
		if (addr.size() != 8 and addr.size() != 32) return {};

		//? The kernel prints each 32 bit word of the address as a host order integer,
		//? so copying the integer back to memory restores the network order bytes
		std::array<unsigned char, 16> bytes{};
		const size_t words = addr.size() / 8;
		for (size_t w = 0; w < words; w++) {
			const auto v = parse_uint<uint32_t>(addr.substr(w * 8, 8), 16).value_or(0);
			std::memcpy(bytes.data() + w * 4, &v, 4);
		}
		std::array<char, INET6_ADDRSTRLEN> buf{};
		const int family = words == 1 ? AF_INET : AF_INET6;
		if (inet_ntop(family, bytes.data(), buf.data(), buf.size()) == nullptr) return {};
		const std::string ip{buf.data()};
		return (family == AF_INET6 ? "[" + ip + "]" : ip) + ":" + std::to_string(port);
	}

	auto read_sockets(const fs::path& proc) -> std::vector<Socket> {
		std::vector<Socket> out;
		for (const std::string proto : {"tcp", "tcp6", "udp", "udp6"}) {
			std::ifstream f(proc / "net" / proto);
			std::string line;
			if (not f.good() or not std::getline(f, line)) continue; // header
			const bool tcp = proto.starts_with("tcp");
			while (std::getline(f, line)) {
				std::istringstream ss(line);
				std::string sl, local, remote, st, txrx, trtm, retr, uid, timeout;
				uint64_t inode{};
				if (not (ss >> sl >> local >> remote >> st >> txrx >> trtm >> retr >> uid >> timeout >> inode)) continue;
				Socket s;
				s.proto = proto;
				s.local = decode_endpoint(local);
				s.remote = decode_endpoint(remote);
				if (tcp) {
					const auto it = tcp_states.find(st);
					s.state = it != tcp_states.end() ? std::string(it->second) : st;
				}
				else s.state = st == "07" ? "UNCONN" : st == "01" ? "ESTABLISHED" : st;
				s.inode = inode;
				out.push_back(std::move(s));
			}
		}
		return out;
	}

	auto socket_owners(const fs::path& proc) -> std::unordered_map<uint64_t, long> {
		std::unordered_map<uint64_t, long> owners;
		std::error_code ec;
		for (const auto& e : fs::directory_iterator(proc, ec)) {
			const auto name = e.path().filename().string();
			const auto pid = parse_uint<long>(name);
			if (not pid) continue;
			std::error_code fec;
			for (const auto& fd : fs::directory_iterator(e.path() / "fd", fec)) {
				std::error_code lec;
				const auto target = fs::read_symlink(fd.path(), lec).string();
				if (lec or not target.starts_with("socket:[") or target.back() != ']') continue;
				if (const auto inode = parse_uint<uint64_t>(std::string_view(target).substr(8, target.size() - 9)))
					owners.try_emplace(*inode, *pid);
			}
		}
		return owners;
	}

	auto Collector::collect(const std::vector<long>& pids, double seconds) -> json {
		json out = json::object();

		const auto stat = read_stat(proc);
		out["sys"] = system_info(proc, etc, last_stat, stat, seconds);
		last_stat = stat;

		out["hwmon"] = to_json(read_hwmon(sys));

		//? Only processes that did I/O this tick, so idle ones cost nothing in a delta
		json io = json::object();
		std::unordered_map<long, IoCounters> now;
		now.reserve(pids.size());
		for (const long pid : pids) {
			const auto c = read_proc_io(proc, pid);
			if (not c) continue;
			now[pid] = *c;
			const auto prev = last_io.find(pid);
			if (prev == last_io.end()) continue;
			const auto r = std::round(rate(prev->second.read, c->read, seconds));
			const auto w = std::round(rate(prev->second.write, c->write, seconds));
			if (r > 0 or w > 0) io[std::to_string(pid)] = {r, w};
		}
		last_io = std::move(now);
		out["proc_io"] = std::move(io);

		if (++sockets_age % std::max(1, sockets_every) == 0) {
			auto list = read_sockets(proc);
			const auto owners = socket_owners(proc);
			json states = json::object();
			size_t udp = 0;
			for (auto& s : list) {
				if (const auto it = owners.find(s.inode); it != owners.end()) s.pid = it->second;
				if (s.proto.starts_with("tcp")) states[s.state] = states.value(s.state, 0) + 1;
				else udp++;
			}
			//? Listening first, then established, then the rest; time-wait is noise
			const auto rank = [](const Socket& s) {
				return s.state == "LISTEN" or s.state == "UNCONN" ? 0 : s.state == "ESTABLISHED" ? 1 : 2;
			};
			std::ranges::stable_sort(list, [&](const Socket& a, const Socket& b) {
				return std::tuple(rank(a), a.proto, a.local) < std::tuple(rank(b), b.proto, b.local);
			});
			constexpr size_t max_list = 1000;
			json arr = json::array();
			size_t listed = 0;
			for (const auto& s : list) {
				if (s.state == "TIME_WAIT" or s.inode == 0) continue;
				if (listed++ == max_list) break;
				arr.push_back({{"proto", s.proto}, {"local", s.local}, {"remote", s.remote}, {"state", s.state}, {"pid", s.pid}});
			}
			sockets = {{"list", std::move(arr)}, {"tcp_states", std::move(states)}, {"udp", udp}, {"total", list.size()}};
		}
		out["sockets"] = sockets;
		return out;
	}
}
