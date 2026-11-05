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

#include "hwlens_client.hpp"

#include <atomic>
#include <cerrno>
#include <chrono>
#include <condition_variable>
#include <csignal>
#include <cstring>
#include <fstream>
#include <mutex>
#include <optional>
#include <stdexcept>
#include <string_view>
#include <thread>
#include <unordered_set>

#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>

#include <fmt/core.h>

#include "hwlens_config.hpp"
#include "hwlens_input.hpp"
#include "hwlens_json.hpp"
#include "hwlens_log.hpp"

using std::string;
using std::string_view;
using Json::json;

namespace Client {
	string socket_path = default_socket;

	namespace {
		//? A daemon line longer than this is a broken stream; a full snapshot is a few hundred KiB
		constexpr size_t max_line = 64 * 1024 * 1024;
		constexpr auto reply_timeout = std::chrono::seconds(3);
		constexpr auto retry_interval = std::chrono::seconds(1);

		//? Socket, replaced on reconnect. Writers hold write_mtx, so the reader never closes it under them.
		std::mutex write_mtx;
		int sock = -1;

		//? Daemon state as the daemon sent it, patched by the reader thread
		std::mutex state_mtx;
		json mirror;
		uint64_t version = 0;
		uint64_t last_seq = 0;

		std::atomic<bool> tick_pending{false};
		std::atomic<bool> state_pending{false};

		//? Last "options" body, sent again after a reconnect: a restarted daemon starts with defaults
		std::mutex options_mtx;
		std::optional<json> last_options;

		//? One request with a reply at a time
		std::mutex request_mtx;
		std::mutex reply_mtx;
		std::condition_variable reply_cv;
		std::optional<json> reply;

		std::thread reader;
		std::atomic<bool> stopping{false};
		std::atomic<bool> reader_done{false};
		std::mutex stop_mtx;
		std::condition_variable stop_cv;

		//? Runner side: the state the collect() functions return
		Json::State state;
		uint64_t applied = 0;
		bool procs_rebuilt = false;

		auto open_socket(const string& path) -> int {
			sockaddr_un addr{};
			addr.sun_family = AF_UNIX;
			if (path.size() >= sizeof(addr.sun_path)) throw std::runtime_error("socket path too long: " + path);
			path.copy(addr.sun_path, path.size());

			const int fd = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
			if (fd < 0) throw std::runtime_error(fmt::format("socket(): {}", strerror(errno)));
			if (connect(fd, reinterpret_cast<const sockaddr*>(&addr), sizeof(addr)) == 0) return fd;

			const int err = errno;
			close(fd);
			if (err == ENOENT or err == ECONNREFUSED)
				throw std::runtime_error(fmt::format(
					"hwlensd is not running (no daemon at {}). Start it with: sudo systemctl enable --now hwlensd.socket", path));
			if (err == EACCES or err == EPERM)
				throw std::runtime_error(fmt::format(
					"No access to {}. Add your user to the hwlens group (sudo usermod -aG hwlens $USER) and log in again", path));
			throw std::runtime_error(fmt::format("Cannot connect to hwlensd at {}: {}", path, strerror(err)));
		}

		auto write_all(int fd, string_view data) -> bool {
			while (not data.empty()) {
				const auto n = ::send(fd, data.data(), data.size(), MSG_NOSIGNAL);
				if (n < 0) {
					if (errno == EINTR) continue;
					return false;
				}
				data.remove_prefix(static_cast<size_t>(n));
			}
			return true;
		}

		auto send_line(const json& msg) -> bool {
			std::lock_guard lock(write_mtx);
			if (sock < 0) return false;
			return write_all(sock, Json::dump(msg) + '\n');
		}

		//* Blocking line reader over one connection
		class Lines {
		public:
			explicit Lines(int fd) : fd(fd) {}

			auto next(string& line) -> bool {
				for (;;) {
					if (const auto nl = buf.find('\n', scanned); nl != string::npos) {
						line.assign(buf, 0, nl);
						buf.erase(0, nl + 1);
						scanned = 0;
						return true;
					}
					scanned = buf.size();
					if (buf.size() > max_line) return false;
					char chunk[65536];
					const auto n = ::read(fd, chunk, sizeof(chunk));
					if (n < 0 and errno == EINTR) continue;
					if (n <= 0) return false;
					buf.append(chunk, static_cast<size_t>(n));
				}
			}

		private:
			int fd;
			string buf;
			size_t scanned = 0;
		};

		std::optional<Lines> lines;

		//* Say "hello" on <fd> and load the full snapshot, then send the settings
		void handshake(int fd) {
			if (not write_all(fd, Json::dump(json{{"hello", {{"v", Json::schema_version}}}}) + '\n'))
				throw std::runtime_error("hwlensd closed the connection");

			lines.emplace(fd);
			for (string line; lines->next(line);) {
				auto msg = json::parse(line, nullptr, false);
				if (not msg.is_object()) throw std::runtime_error("hwlensd sent a malformed message");
				if (const auto it = msg.find("error"); it != msg.end())
					throw std::runtime_error(fmt::format("hwlensd refused the connection: {}", it->dump()));
				if (const auto it = msg.find("full"); it != msg.end()) {
					{
						std::lock_guard lock(state_mtx);
						mirror = std::move(*it);
						last_seq = msg.value("seq", uint64_t{});
						++version;
					}
					//? The daemon answers with a state message once it applied them
					std::lock_guard lock(options_mtx);
					if (last_options.has_value() and not write_all(fd, Json::dump(json{{"options", *last_options}}) + '\n'))
						throw std::runtime_error("hwlensd closed the connection");
					return;
				}
			}
			throw std::runtime_error("hwlensd closed the connection");
		}

		//* Handle one message after the handshake; false when the stream is out of step
		auto handle(const string& line) -> bool {
			auto msg = json::parse(line, nullptr, false);
			if (not msg.is_object()) {
				Logger::warning("hwlensd sent a malformed message");
				return false;
			}
			if (msg.contains("reply")) {
				std::lock_guard lock(reply_mtx);
				reply = std::move(msg);
				reply_cv.notify_all();
				return true;
			}
			if (const auto it = msg.find("error"); it != msg.end()) {
				Logger::warning("hwlensd: {}", it->dump());
				return true;
			}
			const auto seq_it = msg.find("seq");
			if (seq_it == msg.end() or not seq_it->is_number_unsigned()) return true;
			{
				std::lock_guard lock(state_mtx);
				const auto seq = seq_it->get<uint64_t>();
				if (seq != last_seq + 1) {
					Logger::warning("hwlensd message {} after {}: resynchronizing", seq, last_seq);
					return false;
				}
				last_seq = seq;
				if (const auto delta = msg.find("delta"); delta != msg.end()) {
					try {
						Json::patch(mirror, *delta);
					}
					catch (const std::exception& e) {
						Logger::warning("Bad delta from hwlensd: {}", e.what());
						return false;
					}
					++version;
				}
			}
			(msg.value("tick", false) ? tick_pending : state_pending) = true;
			Input::interrupt();
			return true;
		}

		void disconnect() {
			std::lock_guard lock(write_mtx);
			if (sock >= 0) close(sock);
			sock = -1;
		}

		//* Connect again until it works or the client stops
		void reconnect() {
			disconnect();
			{
				std::lock_guard lock(reply_mtx);
				reply_cv.notify_all();
			}
			bool logged = false;
			while (not stopping) {
				try {
					const int fd = open_socket(socket_path);
					try {
						handshake(fd);
					}
					catch (...) {
						close(fd);
						throw;
					}
					{
						std::lock_guard lock(write_mtx);
						sock = fd;
					}
					Logger::info("Reconnected to hwlensd");
					tick_pending = true;
					Input::interrupt();
					return;
				}
				catch (const std::exception& e) {
					if (not logged) Logger::warning("Lost connection to hwlensd, retrying: {}", e.what());
					logged = true;
				}
				std::unique_lock lock(stop_mtx);
				stop_cv.wait_for(lock, retry_interval, [] { return stopping.load(); });
			}
		}

		void read_loop() {
			//? SIGUSR1 must reach the main thread waiting in pselect(), and the signal handlers expect it there
			sigset_t all;
			sigfillset(&all);
			pthread_sigmask(SIG_BLOCK, &all, nullptr);

			for (string line; not stopping;) {
				if (lines->next(line) and handle(line)) continue;
				if (stopping) break;
				reconnect();
			}
			reader_done = true;
		}

		auto request(const json& msg) -> int {
			std::lock_guard serial(request_mtx);
			std::unique_lock lock(reply_mtx);
			reply.reset();
			if (not send_line(msg)) return ENOTCONN;
			if (not reply_cv.wait_for(lock, reply_timeout, [] { return reply.has_value() or stopping; }))
				return ETIMEDOUT;
			if (not reply.has_value()) return ENOTCONN;
			if (reply->value("ok", false)) return 0;
			const auto error = reply->value("error", string{});
			Logger::warning("hwlensd refused {}: {}", msg.dump(), error);
			if (error == "permission denied") return EPERM;
			if (error == "no such process") return ESRCH;
			if (error.contains("range") or error.contains("must be")) return EINVAL;
			return EIO;
		}
	}

	void start() {
		if (not reader.joinable()) reader = std::thread(read_loop);
	}

	void stop() {
		stopping = true;
		{
			std::lock_guard lock(write_mtx);
			if (sock >= 0) shutdown(sock, SHUT_RDWR);
		}
		{
			std::lock_guard lock(stop_mtx);
			stop_cv.notify_all();
		}
		{
			std::lock_guard lock(reply_mtx);
			reply_cv.notify_all();
		}
		//? The reader wakes up from shutdown(); a handshake with a stuck daemon must not hang the exit
		for (int i = 0; i < 50 and reader.joinable() and not reader_done; ++i)
			std::this_thread::sleep_for(std::chrono::milliseconds(10));
		if (reader.joinable()) {
			if (reader_done) reader.join();
			else reader.detach();
		}
		disconnect();
	}

	auto take_events() -> Events {
		return {tick_pending.exchange(false), state_pending.exchange(false)};
	}

	void sync() {
		std::lock_guard lock(state_mtx);
		if (version == applied) return;

		//? Expanded or collapsed tree branches belong to this view, not to the daemon
		std::unordered_set<size_t> collapsed;
		for (const auto& p : state.procs)
			if (p.collapsed) collapsed.insert(p.pid);
		const auto old_cores = Shared::coreCount;
	#ifdef GPU_SUPPORT
		const auto old_gpus = state.gpus.size();
	#endif

		try {
			Json::restore(mirror, state);
		}
		catch (const std::exception& e) {
			Logger::error("Cannot load the state from hwlensd: {}", e.what());
		}
		applied = version;
		procs_rebuilt = true;
		for (auto& p : state.procs)
			if (collapsed.contains(p.pid)) p.collapsed = true;

		bool layout_changed = Shared::coreCount != old_cores;
	#ifdef GPU_SUPPORT
		if (state.gpus.size() != old_gpus) {
			Gpu::set_layout_info(state.gpus);
			layout_changed = true;
		}
	#endif
		if (layout_changed) Runner::coreNum_reset = true;
	}

	void send_options(const Shared::CollectOptions& opts, uint64_t update_ms) {
		auto body = Json::options_json(opts);
		body["update_ms"] = update_ms;
		{
			std::lock_guard lock(options_mtx);
			if (last_options == body) return;
			last_options = body;
		}
		send_line(json{{"options", std::move(body)}});
	}

	void net_select(const string& iface) { send_line(json{{"net", {{"select", iface}}}}); }
	void net_rescale() { send_line(json{{"net", {{"rescale", true}}}}); }
	void net_reset_totals() { send_line(json{{"net", {{"reset_totals", true}}}}); }

	auto send_signal(pid_t pid, int signal) -> int {
		return request(json{{"signal", {{"pid", pid}, {"signal", signal}}}});
	}

	auto set_nice(pid_t pid, int nice) -> int {
		return request(json{{"nice", {{"pid", pid}, {"nice", nice}}}});
	}
}

//* The collector API of the TUI, served from the daemon state

namespace Shared {
	void init() {
		if (const char* env = std::getenv("HWLENSD_SOCKET"); env != nullptr and Client::socket_path == Client::default_socket)
			Client::socket_path = env;
		Client::send_options(collect_opts, static_cast<uint64_t>(Config::getI("update_ms")));

		const int fd = Client::open_socket(Client::socket_path);
		try {
			Client::handshake(fd);
		}
		catch (...) {
			close(fd);
			throw;
		}
		{
			std::lock_guard lock(Client::write_mtx);
			Client::sock = fd;
		}
		Client::sync();
	#ifdef GPU_SUPPORT
		Gpu::set_layout_info(Client::state.gpus);
	#endif
		Runner::coreNum_reset = false;
		Client::tick_pending = true;
		Logger::debug("Connected to hwlensd at {}", Client::socket_path);
	}
}

namespace Cpu {
	auto collect(bool /*no_update*/) -> cpu_info& { return Client::state.cpu; }
	auto get_cpuHz() -> string { return cpuHz; }
	//? The daemon builds the map from the cpu_core_map option; it arrives with the next state
	auto get_core_mapping(const string& /*custom_map*/) -> std::unordered_map<int, int> { return core_mapping; }
}

namespace Mem {
	uint64_t get_totalMem() { return Client::state.total_mem; }
	auto collect(bool /*no_update*/) -> mem_info& { return Client::state.mem; }
}

namespace Net {
	auto collect(bool /*no_update*/) -> net_info& {
		static net_info empty;
		const auto it = current_net.find(selected_iface);
		return it == current_net.end() ? empty : it->second;
	}
}

namespace Proc {
	bool collected_new = false;

	auto collect(bool no_update) -> vector<proc_info>& {
		//? A rebuilt list has no tree or filter state yet, so it always needs a full arrange()
		collected_new = not no_update or Client::procs_rebuilt;
		Client::procs_rebuilt = false;
		return Client::state.procs;
	}
}

#ifdef GPU_SUPPORT
namespace Gpu {
	auto collect(bool /*no_update*/) -> vector<gpu_info>& { return Client::state.gpus; }
}
#endif

namespace Tools {
	double system_uptime() {
		std::ifstream file("/proc/uptime");
		double uptime = 0;
		file >> uptime;
		return uptime;
	}
}
