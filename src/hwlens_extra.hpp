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

#include <cstdint>
#include <filesystem>
#include <optional>
#include <string>
#include <unordered_map>
#include <vector>

#include <nlohmann/json.hpp>

//* Data that only hwlensd clients other than the TUI read: system information, every hwmon
//* sensor, disk I/O per process and sockets. Each collector takes the root of procfs or sysfs,
//* so tests can point it at a fake tree.
namespace Extra {
	using json = nlohmann::json;
	namespace fs = std::filesystem;

	//* /proc/stat counters that only make sense as rates
	struct StatCounters {
		uint64_t ctxt{}, intr{}, forks{}, running{}, blocked{};
	};

	auto read_stat(const fs::path& proc) -> std::optional<StatCounters>;

	//* Static and slow changing system information, plus rates from two /proc/stat readings
	auto system_info(const fs::path& proc, const fs::path& etc, const std::optional<StatCounters>& prev,
		const std::optional<StatCounters>& cur, double seconds) -> json;

	//* One reading of a hwmon attribute, in display units (°C, rpm, V, W, A, %)
	struct Sensor {
		std::string chip, label, kind;
		double value{};
		std::optional<double> max, crit;
	};

	//* Every readable sensor under <sys>/class/hwmon, sorted by chip, kind and index
	auto read_hwmon(const fs::path& sys) -> std::vector<Sensor>;
	auto to_json(const std::vector<Sensor>& sensors) -> json;

	//* Bytes read from and written to storage, from /proc/<pid>/io
	struct IoCounters {
		uint64_t read{}, write{};
	};

	auto read_proc_io(const fs::path& proc, long pid) -> std::optional<IoCounters>;

	//* "addr:port" from the hex form of /proc/net/tcp (IPv4) or tcp6 (IPv6)
	auto decode_endpoint(std::string_view hex) -> std::string;

	struct Socket {
		std::string proto, local, remote, state;
		uint64_t inode{};
		long pid{};
	};

	//* Sockets of /proc/net/{tcp,tcp6,udp,udp6}; pid is 0 until resolved
	auto read_sockets(const fs::path& proc) -> std::vector<Socket>;

	//* inode -> pid for every socket descriptor under /proc/<pid>/fd
	auto socket_owners(const fs::path& proc) -> std::unordered_map<uint64_t, long>;

	//* Keeps the previous readings that rates need; one per daemon
	class Collector {
	public:
		fs::path proc{"/proc"}, sys{"/sys"}, etc{"/etc"};
		//* Sockets and their owners are re-read every this many collections
		int sockets_every = 5;

		//* Sections "sys", "hwmon", "proc_io" and "sockets" for the snapshot
		auto collect(const std::vector<long>& pids, double seconds) -> json;

	private:
		std::optional<StatCounters> last_stat;
		std::unordered_map<long, IoCounters> last_io;
		json sockets = json::object();
		int sockets_age = -1;
	};
}
