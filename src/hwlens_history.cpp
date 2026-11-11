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

#include "hwlens_history.hpp"

#include <algorithm>
#include <cmath>
#include <fstream>
#include <set>

#include "hwlens_log.hpp"

namespace History {
	namespace {
		auto round3(double v) -> double { return std::round(v * 1000) / 1000; }

		//? The <n> processes with the largest <key>, as [pid, name, value] rows
		template <typename Key>
		auto top(const std::vector<std::pair<long, ProcStat>>& all, Key key) -> json {
			std::vector<const std::pair<long, ProcStat>*> order;
			for (const auto& p : all) if (key(p.second) > 0) order.push_back(&p);
			const auto n = std::min(top_count, order.size());
			std::partial_sort(order.begin(), order.begin() + static_cast<std::ptrdiff_t>(n), order.end(),
				[&](const auto* a, const auto* b) { return key(a->second) > key(b->second); });
			json rows = json::array();
			for (size_t i = 0; i < n; i++) rows.push_back({order[i]->first, order[i]->second.name, round3(key(order[i]->second))});
			return rows;
		}

		constexpr int64_t max_points = 3000;
	}

	auto to_json(const Sample& s) -> json {
		json values = json::object();
		for (const auto& [k, v] : s.values) values[k] = round3(v);
		return {{"t", s.t}, {"v", std::move(values)}, {"top", s.top}};
	}

	auto from_json(const json& j) -> Sample {
		Sample s;
		s.t = j.at("t").get<int64_t>();
		for (const auto& [k, v] : j.at("v").items()) if (v.is_number()) s.values[k] = v.get<double>();
		if (const auto it = j.find("top"); it != j.end() and it->is_object()) s.top = *it;
		return s;
	}

	Store::Store(fs::path file, int days) : file(std::move(file)), days(std::max(0, days)) {}

	void Store::load(int64_t now) {
		if (not enabled() or file.empty()) return;
		std::ifstream f(file);
		if (not f.good()) return;
		size_t bad = 0;
		for (std::string line; std::getline(f, line);) {
			if (line.empty()) continue;
			const auto j = json::parse(line, nullptr, false);
			try {
				if (j.is_discarded()) throw std::runtime_error("parse");
				auto s = from_json(j);
				if (not stored.empty() and s.t <= stored.back().t) continue; // clock went back: keep the order
				stored.push_back(std::move(s));
			}
			catch (const std::exception&) {
				bad++;
			}
		}
		const auto before = stored.size();
		prune(now);
		if (bad > 0) Logger::warning("History: skipped {} unreadable lines in {}", bad, file.string());
		if (bad > 0 or stored.size() != before) rewrite();
		Logger::info("History: {} samples from {}", stored.size(), file.string());
	}

	void Store::add(const Tick& tick) {
		if (not enabled()) return;
		const int64_t b = tick.t - (tick.t % bucket_seconds + bucket_seconds) % bucket_seconds;
		if (b != bucket) {
			flush();
			bucket = b;
		}
		ticks++;
		for (const auto& [k, v] : tick.values) if (std::isfinite(v)) sums[k] += v;
		if (const auto it = tick.values.find("cpu"); it != tick.values.end()) cpu_max = std::max(cpu_max, it->second);
		for (const auto& p : tick.procs) {
			auto& acc = procs[p.pid];
			acc.name = p.name;
			acc.cpu_sum += p.cpu;
			acc.mem_max = std::max(acc.mem_max, p.mem);
			acc.io_sum += p.io;
		}
	}

	void Store::flush() {
		if (ticks > 0 and bucket >= 0) {
			Sample s;
			s.t = bucket;
			for (const auto& [k, v] : sums) s.values[k] = v / static_cast<double>(ticks);
			if (sums.contains("cpu")) s.values["cpu_max"] = cpu_max;

			std::vector<std::pair<long, ProcStat>> all;
			all.reserve(procs.size());
			const auto n = static_cast<double>(ticks);
			for (const auto& [pid, acc] : procs)
				all.push_back({pid, ProcStat{pid, acc.name, acc.cpu_sum / n, acc.mem_max, acc.io_sum / n}});
			s.top = {
				{"cpu", top(all, [](const ProcStat& p) { return p.cpu; })},
				{"mem", top(all, [](const ProcStat& p) { return p.mem; })},
				{"io", top(all, [](const ProcStat& p) { return p.io; })},
			};
			if (stored.empty() or s.t > stored.back().t) {
				append(s);
				stored.push_back(std::move(s));
				prune(bucket);
			}
		}
		ticks = 0;
		sums.clear();
		cpu_max = 0;
		procs.clear();
	}

	void Store::prune(int64_t now) {
		const int64_t oldest = now - static_cast<int64_t>(days) * 86400;
		size_t dropped = 0;
		while (not stored.empty() and stored.front().t < oldest) {
			stored.pop_front();
			dropped++;
		}
		//? The file only grows by appending; rewrite it once a day's worth of old samples is gone
		if (dropped > 0 and appended_since_rewrite >= static_cast<size_t>(86400 / bucket_seconds)) rewrite();
	}

	void Store::append(const Sample& s) {
		appended_since_rewrite++;
		if (file.empty()) return;
		std::ofstream f(file, std::ios::app);
		if (not f.good()) {
			Logger::warning("History: cannot write {}", file.string());
			return;
		}
		f << to_json(s).dump() << '\n';
	}

	void Store::rewrite() {
		appended_since_rewrite = 0;
		if (file.empty()) return;
		const auto tmp = fs::path(file.string() + ".tmp");
		{
			std::ofstream f(tmp, std::ios::trunc);
			if (not f.good()) {
				Logger::warning("History: cannot write {}", tmp.string());
				return;
			}
			for (const auto& s : stored) f << to_json(s).dump() << '\n';
		}
		std::error_code ec;
		fs::rename(tmp, file, ec);
		if (ec) Logger::warning("History: cannot replace {}: {}", file.string(), ec.message());
	}

	auto Store::query(const json& body, int64_t now) const -> json {
		json out = {{"reply", "history"}, {"ok", true}, {"days", days}};
		out["oldest"] = stored.empty() ? json(nullptr) : json(stored.front().t);
		out["newest"] = stored.empty() ? json(nullptr) : json(stored.back().t);
		if (not body.is_object()) return {{"reply", "history"}, {"ok", false}, {"error", "history must be an object"}};
		const auto number = [&](const char* key, int64_t fallback) -> int64_t {
			const auto it = body.find(key);
			if (it == body.end()) return fallback;
			if (not it->is_number()) throw std::invalid_argument(std::string(key) + " must be a number");
			return static_cast<int64_t>(it->get<double>());
		};

		try {
			if (body.contains("at")) {
				const auto at = number("at", now);
				if (stored.empty()) {
					out["sample"] = nullptr;
					return out;
				}
				//? The sample nearest to <at>
				auto it = std::ranges::lower_bound(stored, at, {}, &Sample::t);
				if (it == stored.end()) --it;
				else if (it != stored.begin() and at - std::prev(it)->t < it->t - at) --it;
				out["sample"] = to_json(*it);
				return out;
			}

			const auto from = number("from", stored.empty() ? now : stored.front().t);
			const auto to = number("to", now);
			const auto points = std::clamp<int64_t>(number("points", 600), 1, max_points);
			if (to < from) throw std::invalid_argument("to is before from");
			//? Whole minutes per point, so neighbouring requests line up
			const int64_t span = std::max<int64_t>(to - from, bucket_seconds);
			const int64_t step = std::max<int64_t>(1, (span + points * bucket_seconds - 1) / (points * bucket_seconds)) * bucket_seconds;
			out["from"] = from;
			out["to"] = to;
			out["step"] = step;

			json times = json::array();
			std::map<std::string, json> series;
			std::vector<std::map<std::string, std::pair<double, int>>> groups;
			std::vector<int64_t> group_t;
			for (auto it = std::ranges::lower_bound(stored, from, {}, &Sample::t); it != stored.end() and it->t <= to; ++it) {
				const int64_t g = from + (it->t - from) / step * step;
				if (group_t.empty() or group_t.back() != g) {
					group_t.push_back(g);
					groups.emplace_back();
				}
				for (const auto& [k, v] : it->values) {
					auto& [acc, n] = groups.back()[k];
					//? Peaks stay peaks when points are merged
					acc = k.ends_with("_max") ? (n == 0 ? v : std::max(acc, v)) : acc + v;
					n++;
				}
			}
			std::set<std::string> names;
			for (const auto& g : groups) for (const auto& [k, _] : g) names.insert(k);
			for (const auto& k : names) series[k] = json::array();
			for (size_t i = 0; i < groups.size(); i++) {
				times.push_back(group_t[i]);
				for (const auto& k : names) {
					const auto it = groups[i].find(k);
					if (it == groups[i].end()) { series[k].push_back(nullptr); continue; }
					const auto [acc, n] = it->second;
					series[k].push_back(round3(k.ends_with("_max") ? acc : acc / n));
				}
			}
			out["t"] = std::move(times);
			out["series"] = std::move(series);
			return out;
		}
		catch (const std::exception& e) {
			return {{"reply", "history"}, {"ok", false}, {"error", e.what()}};
		}
	}
}
