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

//* hwlensd: collects system data once per tick and serves it to hwlens clients over a unix socket.
//*
//* Protocol: one JSON object per line in both directions, see docs/protocol.md.

#include <algorithm>
#include <cerrno>
#include <chrono>
#include <csignal>
#include <cstring>
#include <fstream>
#include <iostream>
#include <list>
#include <optional>
#include <string>
#include <string_view>
#include <thread>
#include <vector>

#include <fcntl.h>
#include <poll.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <unistd.h>

#include <fmt/core.h>

#include "config.h"
#include "hwlens_json.hpp"
#include "hwlens_log.hpp"
#include "hwlens_server.hpp"
#include "hwlens_shared.hpp"
#include "hwlens_tools.hpp"

using std::string;
using std::string_view;
using Json::json;

namespace Global {
	const string Version{HWLENS_VERSION};
	uid_t real_uid, set_uid;
	atomic<bool> resized (false);
	atomic<bool> init_conf (false);
	string exit_error_msg;
}

namespace {
	volatile std::sig_atomic_t quit_signal = 0;

	void on_signal(int sig) { quit_signal = sig; }

	//* Collection state, shared by all clients
	struct Collector {
		Json::State state;

		//* Run every collector. <no_update> only re-derives state (net selection, process details).
		void collect(bool no_update) {
			try {
			#ifdef GPU_SUPPORT
				if (not no_update) Proc::gpu_mem.clear();
				state.gpus = Gpu::collect(no_update);
			#endif
				state.cpu = Cpu::collect(no_update);
				state.mem = Mem::collect(no_update);
				Net::collect(no_update);
				state.procs = Proc::collect(no_update);
				state.total_mem = Mem::get_totalMem();
			}
			catch (const std::exception& e) {
				Logger::error("Collection failed: {}", e.what());
			}
		}

		//* Snapshot of the current state; one shot flags are cleared once they are in a snapshot
		auto snapshot() -> json {
			auto snap = Json::snapshot(state);
			Mem::redraw = Net::redraw = Proc::redraw = false;
			return snap;
		}
	};

	//* Daemon settings from /etc/hwlens/hwlensd.conf
	struct Settings {
		uint64_t update_ms = 1000;
		string log_level = "WARNING";
	};

	auto read_settings(const string& path, Settings& settings) -> bool {
		std::ifstream file(path);
		if (not file.good()) return false;
		for (string line; std::getline(file, line);) {
			const auto trimmed = Tools::trim(line);
			if (trimmed.empty() or trimmed.front() == '#') continue;
			const auto eq = trimmed.find('=');
			if (eq == string_view::npos) continue;
			const string key{Tools::trim(trimmed.substr(0, eq))};
			string value{Tools::trim(trimmed.substr(eq + 1))};
			if (value.size() >= 2 and value.front() == '"' and value.back() == '"') value = value.substr(1, value.size() - 2);
			try {
				if (key == "update_ms") settings.update_ms = std::clamp<uint64_t>(std::stoull(value), Server::min_update_ms, Server::max_update_ms);
				else if (key == "log_level") settings.log_level = value;
				else Logger::warning("{}: unknown setting {}", path, key);
			}
			catch (const std::exception&) {
				Logger::warning("{}: bad value for {}", path, key);
			}
		}
		return true;
	}

	//* Listening socket passed by systemd socket activation, or -1
	auto systemd_socket() -> int {
		const char* pid = std::getenv("LISTEN_PID");
		const char* fds = std::getenv("LISTEN_FDS");
		if (pid == nullptr or fds == nullptr) return -1;
		try {
			if (std::stol(pid) != getpid() or std::stoi(fds) < 1) return -1;
		}
		catch (const std::exception&) { return -1; }
		unsetenv("LISTEN_PID");
		unsetenv("LISTEN_FDS");
		unsetenv("LISTEN_FDNAMES");
		constexpr int first_fd = 3;
		fcntl(first_fd, F_SETFD, FD_CLOEXEC);
		fcntl(first_fd, F_SETFL, fcntl(first_fd, F_GETFL) | O_NONBLOCK);
		return first_fd;
	}

	auto bind_socket(const string& path, mode_t mode) -> int {
		sockaddr_un addr{};
		addr.sun_family = AF_UNIX;
		if (path.size() >= sizeof(addr.sun_path)) throw std::runtime_error("socket path too long: " + path);
		std::memcpy(addr.sun_path, path.c_str(), path.size() + 1);

		const int fd = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC | SOCK_NONBLOCK, 0);
		if (fd < 0) throw std::runtime_error(fmt::format("socket(): {}", strerror(errno)));
		unlink(path.c_str());
		//? Create the socket without access for others, then set the requested mode
		const auto old_umask = umask(0177);
		const int bound = bind(fd, reinterpret_cast<const sockaddr*>(&addr), sizeof(addr));
		umask(old_umask);
		if (bound != 0) throw std::runtime_error(fmt::format("bind({}): {}", path, strerror(errno)));
		if (chmod(path.c_str(), mode) != 0) throw std::runtime_error(fmt::format("chmod({}): {}", path, strerror(errno)));
		if (listen(fd, 16) != 0) throw std::runtime_error(fmt::format("listen(): {}", strerror(errno)));
		return fd;
	}

	void usage() {
		fmt::println("Usage: hwlensd [options]\n\n"
					 "Options:\n"
					 "  --socket PATH    Listen on PATH instead of the socket passed by systemd\n"
					 "  --socket-mode M  Octal permissions of the --socket file (default 0660)\n"
					 "  --config PATH    Settings file (default /etc/hwlens/hwlensd.conf)\n"
					 "  --dump-json      Collect twice, one second apart, print the snapshot as JSON and exit\n"
					 "  --log-level LVL  DISABLED, ERROR, WARNING, INFO or DEBUG, logs go to stderr\n"
					 "  -h, --help       Show this help\n"
					 "  -V, --version    Show the version");
	}
}

int main(int argc, char** argv) {
	bool dump_json = false;
	std::optional<string> log_level;
	string socket_path;
	mode_t socket_mode = 0660;
	string config_path = "/etc/hwlens/hwlensd.conf";
	const std::vector<string_view> args(argv + 1, argv + argc);
	for (size_t i = 0; i < args.size(); i++) {
		const auto arg = args[i];
		const bool has_value = i + 1 < args.size();
		if (arg == "--dump-json") dump_json = true;
		else if (arg == "--log-level" and has_value) log_level = string{args[++i]};
		else if (arg == "--socket" and has_value) socket_path = args[++i];
		else if (arg == "--socket-mode" and has_value) socket_mode = static_cast<mode_t>(std::strtoul(string{args[++i]}.c_str(), nullptr, 8) & 0777);
		else if (arg == "--config" and has_value) config_path = args[++i];
		else if (arg == "-h" or arg == "--help") { usage(); return 0; }
		else if (arg == "-V" or arg == "--version") { fmt::println("hwlensd version: {}", Global::Version); return 0; }
		else {
			fmt::println(std::cerr, "hwlensd: unknown or incomplete option {}", arg);
			usage();
			return 1;
		}
	}

	Global::real_uid = getuid();
	Global::set_uid = geteuid();
	Logger::init("-");
	Logger::set_log_level("WARNING");
	Settings settings;
	read_settings(config_path, settings);
	Logger::set_log_level(log_level.value_or(settings.log_level));

	try {
		Shared::init();
	}
	catch (const std::exception& e) {
		Logger::error("Init failed: {}", e.what());
		fmt::println(std::cerr, "hwlensd: init failed: {}", e.what());
		return 1;
	}

	Collector collector;
	if (dump_json) {
		//? Rates (cpu, disks, net) need two samples
		collector.collect(false);
		std::this_thread::sleep_for(std::chrono::seconds(1));
		collector.collect(false);
		fmt::println("{}", Json::dump(collector.snapshot()));
		return 0;
	}

	int listen_fd = -1;
	try {
		listen_fd = socket_path.empty() ? systemd_socket() : bind_socket(socket_path, socket_mode);
	}
	catch (const std::exception& e) {
		Logger::error("{}", e.what());
		return 1;
	}
	if (listen_fd < 0) {
		fmt::println(std::cerr, "hwlensd: no socket, start it through hwlensd.socket or pass --socket PATH");
		return 1;
	}

	struct sigaction sa{};
	sa.sa_handler = on_signal;
	sigaction(SIGTERM, &sa, nullptr);
	sigaction(SIGINT, &sa, nullptr);
	std::signal(SIGPIPE, SIG_IGN);

	Server::Server server(listen_fd, settings.update_ms);
	server.on_collect = [&collector](bool no_update) { collector.collect(no_update); };
	server.on_snapshot = [&collector] { return collector.snapshot(); };
	Logger::info("hwlensd {} started, update interval {} ms", Global::Version, settings.update_ms);

	collector.collect(false);
	server.run(quit_signal);

	Logger::info("Stopping on signal {}", static_cast<int>(quit_signal));
	if (not socket_path.empty()) unlink(socket_path.c_str());
#ifdef GPU_SUPPORT
	Gpu::Nvml::shutdown();
	Gpu::Rsmi::shutdown();
	Gpu::Asysfs::shutdown();
#endif
	return 0;
}
