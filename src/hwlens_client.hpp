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
#include <string>

#include <sys/types.h>

#include "hwlens_shared.hpp"

//* Connection of the TUI to hwlensd, see docs/protocol.md.
//*
//* The TUI does not read /proc or /sys itself. A reader thread keeps a copy of the daemon's
//* snapshot up to date, and the collect() functions of Cpu, Mem, Net, Proc and Gpu return the
//* state from that copy. Shared::init() connects and loads the first snapshot.
namespace Client {
	constexpr auto default_socket = "/run/hwlensd/hwlensd.sock";

	//* Socket path, from --socket or $HWLENSD_SOCKET, else default_socket
	extern std::string socket_path;

	//* Start the reader thread. Call once signal handlers are set up: it wakes the main loop with SIGUSR1.
	void start();

	//* Stop the reader thread and close the connection
	void stop();

	struct Events {
		bool tick = false;   //? new samples: draw a new frame
		bool state = false;  //? derived state changed: redraw without new samples
	};

	//* Events that arrived since the last call
	auto take_events() -> Events;

	//* Load the newest daemon state into the collector globals. Called by the runner at the start of a cycle.
	void sync();

	//* Send collection settings and the tick interval if they differ from what was sent last
	void send_options(const Shared::CollectOptions& opts, uint64_t update_ms);

	//* Net box actions, applied by the daemon for every client
	void net_select(const std::string& iface);
	void net_rescale();
	void net_reset_totals();

	//* Process actions, run by the daemon with its own permission checks.
	//* Return 0 or an errno value (EPERM, ESRCH, EINVAL, ENOTCONN, ETIMEDOUT, EIO).
	auto send_signal(pid_t pid, int signal) -> int;
	auto set_nice(pid_t pid, int nice) -> int;
}
