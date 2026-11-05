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

#include "hwlens_actions.hpp"

#include <cerrno>
#include <csignal>
#include <cstring>
#include <fstream>
#include <optional>
#include <sstream>

#include <poll.h>
#include <sys/resource.h>
#include <sys/syscall.h>
#include <unistd.h>

#include <fmt/core.h>

#include "hwlens_log.hpp"

using std::string;

namespace Actions {
	namespace {
		//? Closes a pidfd on scope exit
		struct PidFd {
			int fd = -1;
			explicit PidFd(pid_t pid) : fd(static_cast<int>(syscall(SYS_pidfd_open, pid, 0))) {}
			~PidFd() { if (fd >= 0) close(fd); }
			PidFd(const PidFd&) = delete;
			PidFd& operator=(const PidFd&) = delete;
			//? A pidfd becomes readable once the process has exited
			[[nodiscard]] auto alive() const -> bool {
				pollfd p{fd, POLLIN, 0};
				return poll(&p, 1, 0) == 0;
			}
		};

		struct Owner { uid_t real, saved; };

		//? Real and saved uid from /proc/<pid>/status
		auto owner(pid_t pid) -> std::optional<Owner> {
			std::ifstream status(fmt::format("/proc/{}/status", pid));
			for (string line; std::getline(status, line);) {
				if (not line.starts_with("Uid:")) continue;
				std::istringstream fields(line.substr(4));
				uid_t real, effective, saved;
				if (fields >> real >> effective >> saved) return Owner{real, saved};
			}
			return std::nullopt;
		}

		//? Integers built in C++ are signed json numbers, parsed ones unsigned
		auto read_int(const Json::json& body, const char* key) -> std::optional<int64_t> {
			if (not body.is_object()) return std::nullopt;
			const auto it = body.find(key);
			if (it == body.end() or not it->is_number_integer()) return std::nullopt;
			if (it->is_number_unsigned() and it->get<uint64_t>() > static_cast<uint64_t>(INT64_MAX)) return std::nullopt;
			return it->get<int64_t>();
		}

		auto read_pid(const Json::json& body) -> pid_t {
			const auto pid = read_int(body, "pid");
			return pid.has_value() and *pid > 0 and *pid <= 4'194'304 ? static_cast<pid_t>(*pid) : 0;
		}
	}

	auto may_act(uid_t client_uid, uid_t real, uid_t saved) -> bool {
		return client_uid == 0 or client_uid == real or client_uid == saved;
	}

	auto may_renice(uid_t client_uid, int current, int wanted) -> bool {
		if (wanted < -20 or wanted > 19) return false;
		return client_uid == 0 or wanted >= current;
	}

	auto send_signal(const Json::json& body, uid_t client_uid, pid_t client_pid) -> string {
		const pid_t pid = read_pid(body);
		if (pid == 0) return "pid must be a process id";
		const auto sig_value = read_int(body, "signal");
		if (not sig_value.has_value()) return "signal must be a number";
		if (*sig_value < 1 or *sig_value > SIGRTMAX) return "signal out of range";
		const int sig = static_cast<int>(*sig_value);

		//? Pin the process first: the checks and the signal then apply to the same process even if the pid is reused
		const PidFd pidfd(pid);
		if (pidfd.fd < 0) return "no such process";
		const auto own = owner(pid);
		if (not own.has_value() or not pidfd.alive()) return "no such process";
		if (not may_act(client_uid, own->real, own->saved)) {
			Logger::warning("Denied signal {} to pid {} (uid {}) for client uid {} pid {}", sig, pid, own->real, client_uid, client_pid);
			return "permission denied";
		}
		if (syscall(SYS_pidfd_send_signal, pidfd.fd, sig, nullptr, 0) != 0) return strerror(errno);
		Logger::info("Client uid {} pid {} sent signal {} to pid {} (uid {})", client_uid, client_pid, sig, pid, own->real);
		return {};
	}

	auto set_nice(const Json::json& body, uid_t client_uid, pid_t client_pid) -> string {
		const pid_t pid = read_pid(body);
		if (pid == 0) return "pid must be a process id";
		const auto nice_value = read_int(body, "nice");
		if (not nice_value.has_value()) return "nice must be a number";
		const auto wanted = *nice_value;
		if (wanted < -20 or wanted > 19) return "nice out of range";

		const PidFd pidfd(pid);
		if (pidfd.fd < 0) return "no such process";
		const auto own = owner(pid);
		if (not own.has_value()) return "no such process";
		errno = 0;
		const int current = getpriority(PRIO_PROCESS, static_cast<id_t>(pid));
		if (errno != 0) return strerror(errno);
		if (not may_act(client_uid, own->real, own->saved) or not may_renice(client_uid, current, static_cast<int>(wanted))) {
			Logger::warning("Denied nice {} for pid {} (uid {}, nice {}) to client uid {} pid {}", wanted, pid, own->real, current, client_uid, client_pid);
			return "permission denied";
		}
		//? setpriority() has no pidfd form: only go ahead while the pinned process still runs
		if (not pidfd.alive()) return "no such process";
		if (setpriority(PRIO_PROCESS, static_cast<id_t>(pid), static_cast<int>(wanted)) != 0) return strerror(errno);
		Logger::info("Client uid {} pid {} set nice {} (was {}) for pid {} (uid {})", client_uid, client_pid, wanted, current, pid, own->real);
		return {};
	}
}
