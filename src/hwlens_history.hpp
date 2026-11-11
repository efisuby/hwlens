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
#include <deque>
#include <filesystem>
#include <map>
#include <string>
#include <unordered_map>
#include <vector>

#include <nlohmann/json.hpp>

//* Long term history kept by hwlensd: one sample per minute with the average of each metric,
//* the peak CPU load and the busiest processes of that minute. Samples are appended to a JSON
//* lines file and survive restarts; the "history" request reads them (docs/protocol.md).
namespace History {
	using json = nlohmann::json;
	namespace fs = std::filesystem;

	constexpr int64_t bucket_seconds = 60;
	//* Processes kept per minute and per ranking (cpu, mem, io)
	constexpr size_t top_count = 5;

	struct ProcStat {
		long pid{};
		std::string name;
		double cpu{};  // percent of one core
		double mem{};  // bytes
		double io{};   // bytes per second
	};

	//* One collection: unix time, metric values and the processes
	struct Tick {
		int64_t t{};
		std::map<std::string, double> values;
		std::vector<ProcStat> procs;
	};

	struct Sample {
		int64_t t{};
		std::map<std::string, double> values;
		json top = json::object();
	};

	auto to_json(const Sample& s) -> json;
	auto from_json(const json& j) -> Sample;

	class Store {
	public:
		//* <file> empty: keep the history in memory only. <days> 0: no history at all.
		Store(fs::path file, int days);

		//* Reads the file, dropping samples older than the retention
		void load(int64_t now);
		//* Adds a collection; finishing a minute stores its sample
		void add(const Tick& tick);
		//* Answer to a "history" request body
		auto query(const json& body, int64_t now) const -> json;

		auto samples() const -> const std::deque<Sample>& { return stored; }
		auto enabled() const -> bool { return days > 0; }

	private:
		struct ProcAcc {
			std::string name;
			double cpu_sum{}, mem_max{}, io_sum{};
		};

		fs::path file;
		int days;
		std::deque<Sample> stored;
		size_t appended_since_rewrite = 0;

		int64_t bucket = -1;
		size_t ticks = 0;
		std::map<std::string, double> sums;
		double cpu_max = 0;
		std::unordered_map<long, ProcAcc> procs;

		void flush();
		void prune(int64_t now);
		void append(const Sample& s);
		void rewrite();
	};
}
