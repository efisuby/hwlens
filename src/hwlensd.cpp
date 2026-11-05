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

//* hwlensd: collects system data and serves snapshots to hwlens clients

#include <chrono>
#include <iostream>
#include <string>
#include <string_view>
#include <thread>
#include <vector>

#include <fmt/core.h>

#include "config.h"
#include "hwlens_json.hpp"
#include "hwlens_log.hpp"
#include "hwlens_shared.hpp"

using std::string;
using std::string_view;

namespace Global {
	const string Version{HWLENS_VERSION};
	uid_t real_uid, set_uid;
	atomic<bool> resized (false);
	atomic<bool> init_conf (false);
	string exit_error_msg;
}

namespace Daemon {
	//* Run every collector once and return the results
	auto collect() -> Json::State {
		Json::State state;
	#ifdef GPU_SUPPORT
		Proc::gpu_mem.clear();
		state.gpus = Gpu::collect();
	#endif
		state.cpu = Cpu::collect();
		state.mem = Mem::collect();
		Net::collect();
		state.procs = Proc::collect();
		state.total_mem = Mem::get_totalMem();
		return state;
	}
}

static void usage() {
	fmt::println("Usage: hwlensd [options]\n\n"
				 "Options:\n"
				 "  --dump-json      Collect twice, one second apart, print the snapshot as JSON and exit\n"
				 "  --log-level LVL  DISABLED, ERROR, WARNING, INFO or DEBUG (default WARNING), logs go to stderr\n"
				 "  -h, --help       Show this help\n"
				 "  -V, --version    Show the version");
}

int main(int argc, char** argv) {
	bool dump_json = false;
	string log_level = "WARNING";
	const std::vector<string_view> args(argv + 1, argv + argc);
	for (size_t i = 0; i < args.size(); i++) {
		const auto arg = args[i];
		if (arg == "--dump-json") dump_json = true;
		else if (arg == "--log-level" and i + 1 < args.size()) log_level = args[++i];
		else if (arg == "-h" or arg == "--help") { usage(); return 0; }
		else if (arg == "-V" or arg == "--version") { fmt::println("hwlensd version: {}", Global::Version); return 0; }
		else {
			fmt::println(std::cerr, "hwlensd: unknown option {}", arg);
			usage();
			return 1;
		}
	}

	Global::real_uid = getuid();
	Global::set_uid = geteuid();
	Logger::init("-");
	Logger::set_log_level(log_level);

	try {
		Shared::init();
	}
	catch (const std::exception& e) {
		fmt::println(std::cerr, "hwlensd: init failed: {}", e.what());
		return 1;
	}

	if (dump_json) {
		//? Rates (cpu, disks, net) need two samples
		Daemon::collect();
		std::this_thread::sleep_for(std::chrono::seconds(1));
		const auto state = Daemon::collect();
		fmt::println("{}", Json::snapshot(state).dump());
		return 0;
	}

	usage();
	return 1;
}
