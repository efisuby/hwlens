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

#include <csignal>
#include <functional>
#include <list>
#include <optional>
#include <string>

#include <sys/types.h>

#include "hwlens_json.hpp"

//* Socket server of hwlensd, see docs/protocol.md
namespace Server {
	constexpr uint64_t min_update_ms = 100;
	constexpr uint64_t max_update_ms = 86'400'000;
	//? Limits for requests from clients, which the daemon parses as root
	constexpr size_t max_clients = 32;
	constexpr size_t max_line = 64 * 1024;
	constexpr size_t max_depth = 8;
	constexpr size_t max_string = 256;
	constexpr size_t max_pending_output = 32 * 1024 * 1024;
	constexpr int max_errors = 16;

	//* Apply the "options" request <opts> to Shared::collect_opts and <update_ms>.
	//* Returns an error message, or an empty string when everything was applied.
	auto apply_options(const Json::json& opts, uint64_t& update_ms) -> std::string;

	//* Nesting depth of <line> outside of strings; parsing is skipped for too deep requests
	auto json_depth(std::string_view line) -> size_t;

	class Server {
	public:
		Server(int listen_fd, uint64_t update_ms);
		~Server();
		Server(const Server&) = delete;
		Server& operator=(const Server&) = delete;

		//* Collect new data (false) or re-derive state without new samples (true)
		std::function<void(bool no_update)> on_collect;
		//* Snapshot of the collected state
		std::function<Json::json()> on_snapshot;
		//* Answer to a "history" request body; unset: the request is refused
		std::function<Json::json(const Json::json&)> on_history;

		//* Serve until <quit> is set by a signal handler
		void run(const volatile std::sig_atomic_t& quit);

	private:
		struct Client {
			int fd;
			uid_t uid;
			gid_t gid;
			pid_t pid;
			std::string in;
			std::string out;
			bool subscribed = false;
			bool closing = false;
			int errors = 0;
		};

		int listen_fd;
		uint64_t update_ms;
		uint64_t next_tick;
		std::list<Client> clients;
		//? Snapshot that every subscribed client holds, empty while nobody is subscribed
		std::optional<Json::json> mirrored;
		//? Number of the last broadcast; "full" and "tick" messages carry it so clients can spot gaps
		uint64_t seq = 0;

		void accept_clients();
		void read_client(Client& client);
		void write_client(Client& client);
		void handle_line(Client& client, std::string_view line);
		void handle_request(Client& client, const Json::json& request);
		void send(Client& client, const std::string& line);
		void reply_error(Client& client, std::string_view what);
		void tick();
		//* Send the changes since the last broadcast; <tick> tells that they hold new samples
		void broadcast(bool tick);
		auto subscribers() const -> size_t;
	};
}
