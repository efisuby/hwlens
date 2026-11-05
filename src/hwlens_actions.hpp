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

#include <string>

#include <sys/types.h>

#include "hwlens_json.hpp"

//* Process actions hwlensd runs for its clients. The daemon has CAP_KILL and CAP_SYS_NICE, so it
//* applies the same rules the kernel applies to an unprivileged caller: a client may act on its
//* own processes only, and may not lower a nice value; a root client may do anything.
namespace Actions {
	//* {"pid": N, "signal": S}. Returns an error message, empty on success.
	auto send_signal(const Json::json& body, uid_t client_uid, pid_t client_pid) -> std::string;

	//* {"pid": N, "nice": V}. Returns an error message, empty on success.
	auto set_nice(const Json::json& body, uid_t client_uid, pid_t client_pid) -> std::string;

	//* May <client_uid> act on a process with real uid <real> and saved uid <saved>
	auto may_act(uid_t client_uid, uid_t real, uid_t saved) -> bool;

	//* May <client_uid> change a nice value from <current> to <wanted>
	auto may_renice(uid_t client_uid, int current, int wanted) -> bool;
}
