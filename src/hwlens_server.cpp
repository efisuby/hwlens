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

#include "hwlens_server.hpp"

#include <algorithm>
#include <array>
#include <cerrno>
#include <cstring>
#include <vector>

#include <poll.h>
#include <sys/socket.h>
#include <unistd.h>

#include <fmt/core.h>

#include "hwlens_actions.hpp"
#include "hwlens_log.hpp"
#include "hwlens_shared.hpp"
#include "hwlens_tools.hpp"

using std::string;
using std::string_view;
using Json::json;

namespace Server {
	namespace {
		auto printable(string_view s) -> bool {
			return std::ranges::all_of(s, [](unsigned char c) { return c >= 0x20 and c != 0x7f; });
		}

		//? Only the forms the collectors understand, anything else could make them throw
		auto valid_string_option(string_view name, string_view value) -> bool {
			if (value.size() > max_string or not printable(value)) return false;
			if (name == "freq_mode")
				return Tools::is_in(value, "first", "range", "lowest", "highest", "average", "clusters");
			if (name == "cpu_core_map")
				return std::ranges::all_of(value, [](char c) { return std::isdigit(static_cast<unsigned char>(c)) or c == ':' or c == ' '; });
			if (name == "disks_filter")
				return value.empty() or not Tools::trim(value).empty();
			return true;
		}

		auto now_ms() -> uint64_t { return Tools::time_ms(); }

		//? Integers built in C++ are signed json numbers, parsed ones unsigned: accept both
		auto non_negative(const json& v) -> bool {
			return v.is_number_unsigned() or (v.is_number_integer() and v.get<int64_t>() >= 0);
		}
	}

	auto json_depth(string_view line) -> size_t {
		size_t depth = 0, deepest = 0;
		bool in_string = false, escaped = false;
		for (const char c : line) {
			if (in_string) {
				if (escaped) escaped = false;
				else if (c == '\\') escaped = true;
				else if (c == '"') in_string = false;
				continue;
			}
			if (c == '"') in_string = true;
			else if (c == '{' or c == '[') deepest = std::max(deepest, ++depth);
			else if ((c == '}' or c == ']') and depth > 0) --depth;
		}
		return deepest;
	}

	auto apply_options(const json& opts, uint64_t& update_ms) -> string {
		if (not opts.is_object()) return "options must be an object";
		//? Validate everything first, so that a bad request changes nothing
		Shared::CollectOptions next = Shared::collect_opts;
		uint64_t next_update = update_ms;
		for (const auto& [key, value] : opts.items()) {
			if (key == "update_ms") {
				if (not non_negative(value)) return "update_ms must be a positive integer";
				next_update = std::clamp<uint64_t>(value.get<uint64_t>(), min_update_ms, max_update_ms);
				continue;
			}
			if (key == "detailed_pid") {
				if (not non_negative(value)) return "detailed_pid must be a positive integer";
				next.detailed_pid = value.get<size_t>();
				continue;
			}
			if (const auto s = std::ranges::find(Json::string_options, key, &Json::StringOption::name); s != Json::string_options.end()) {
				if (not value.is_string() or not valid_string_option(key, value.get_ref<const string&>()))
					return fmt::format("bad value for {}", key);
				next.*(s->field) = value.get<string>();
				continue;
			}
			if (const auto b = std::ranges::find(Json::bool_options, key, &Json::BoolOption::name); b != Json::bool_options.end()) {
				if (not value.is_boolean()) return fmt::format("{} must be a boolean", key);
				next.*(b->field) = value.get<bool>();
				continue;
			}
			return fmt::format("unknown option {}", key.substr(0, max_string));
		}
		const bool core_map_changed = next.cpu_core_map != Shared::collect_opts.cpu_core_map;
		Shared::collect_opts = std::move(next);
		update_ms = next_update;
		if (core_map_changed) Cpu::core_mapping = Cpu::get_core_mapping(Shared::collect_opts.cpu_core_map);
		return {};
	}

	Server::Server(int listen_fd, uint64_t update_ms)
		: listen_fd(listen_fd), update_ms(update_ms), next_tick(now_ms() + update_ms) {}

	Server::~Server() {
		for (auto& c : clients) close(c.fd);
		close(listen_fd);
	}

	auto Server::subscribers() const -> size_t {
		return std::ranges::count_if(clients, [](const Client& c) { return c.subscribed and not c.closing; });
	}

	void Server::run(const volatile std::sig_atomic_t& quit) {
		std::vector<pollfd> fds;
		while (quit == 0) {
			fds.clear();
			fds.push_back({listen_fd, POLLIN, 0});
			for (const auto& c : clients) {
				short events = c.closing ? 0 : POLLIN;
				if (not c.out.empty()) events |= POLLOUT;
				fds.push_back({c.fd, events, 0});
			}
			const auto now = now_ms();
			const int timeout = next_tick > now ? static_cast<int>(std::min<uint64_t>(next_tick - now, 1000)) : 0;
			if (poll(fds.data(), fds.size(), timeout) < 0) {
				if (errno == EINTR) continue;
				Logger::error("poll(): {}", strerror(errno));
				break;
			}

			if (fds[0].revents & POLLIN) accept_clients();
			size_t i = 1;
			for (auto& c : clients) {
				if (i >= fds.size()) break;
				const auto revents = fds[i++].revents;
				if (revents & (POLLERR | POLLNVAL)) c.closing = true, c.out.clear();
				if (revents & (POLLIN | POLLHUP)) read_client(c);
				if (revents & POLLOUT) write_client(c);
			}
			//? Drop closed clients once their last message is out
			std::erase_if(clients, [](const Client& c) {
				if (c.closing and c.out.empty()) {
					Logger::debug("Client pid {} uid {} disconnected", c.pid, c.uid);
					close(c.fd);
					return true;
				}
				return false;
			});

			if (now_ms() >= next_tick) {
				tick();
				const auto after = now_ms();
				next_tick += update_ms;
				if (next_tick <= after) next_tick = after + update_ms;
			}
		}
	}

	void Server::accept_clients() {
		while (true) {
			const int fd = accept4(listen_fd, nullptr, nullptr, SOCK_NONBLOCK | SOCK_CLOEXEC);
			if (fd < 0) {
				if (errno != EAGAIN and errno != EWOULDBLOCK and errno != EINTR)
					Logger::warning("accept(): {}", strerror(errno));
				return;
			}
			ucred cred{};
			socklen_t len = sizeof(cred);
			if (getsockopt(fd, SOL_SOCKET, SO_PEERCRED, &cred, &len) != 0) {
				close(fd);
				continue;
			}
			if (clients.size() >= max_clients) {
				Logger::warning("Refusing client pid {} uid {}: too many clients", cred.pid, cred.uid);
				close(fd);
				continue;
			}
			Logger::debug("Client pid {} uid {} connected", cred.pid, cred.uid);
			clients.push_back({fd, cred.uid, cred.gid, cred.pid, {}, {}});
		}
	}

	void Server::read_client(Client& client) {
		std::array<char, 16 * 1024> buf;
		while (not client.closing) {
			const auto n = recv(client.fd, buf.data(), buf.size(), 0);
			if (n == 0) { client.closing = true; client.out.clear(); return; }
			if (n < 0) {
				if (errno == EAGAIN or errno == EWOULDBLOCK) break;
				if (errno == EINTR) continue;
				client.closing = true;
				client.out.clear();
				return;
			}
			client.in.append(buf.data(), static_cast<size_t>(n));
			size_t start = 0;
			for (size_t nl; (nl = client.in.find('\n', start)) != string::npos; start = nl + 1) {
				handle_line(client, string_view(client.in).substr(start, nl - start));
				if (client.closing) return;
			}
			client.in.erase(0, start);
			if (client.in.size() > max_line) {
				reply_error(client, "request too long");
				client.closing = true;
				return;
			}
		}
	}

	void Server::write_client(Client& client) {
		while (not client.out.empty()) {
			const auto n = ::send(client.fd, client.out.data(), client.out.size(), MSG_NOSIGNAL);
			if (n < 0) {
				if (errno == EAGAIN or errno == EWOULDBLOCK) return;
				if (errno == EINTR) continue;
				client.out.clear();
				client.closing = true;
				return;
			}
			client.out.erase(0, static_cast<size_t>(n));
		}
	}

	void Server::send(Client& client, const string& line) {
		if (client.closing and client.out.empty()) return;
		if (client.out.size() + line.size() > max_pending_output) {
			Logger::warning("Dropping client pid {} uid {}: not reading its data", client.pid, client.uid);
			client.out.clear();
			client.closing = true;
			return;
		}
		client.out += line;
		client.out += '\n';
		write_client(client);
	}

	void Server::reply_error(Client& client, string_view what) {
		send(client, Json::dump(json{{"error", what}}));
		if (++client.errors >= max_errors) client.closing = true;
	}

	void Server::handle_line(Client& client, string_view line) {
		if (line.size() > max_line) return reply_error(client, "request too long");
		if (Tools::trim(line).empty()) return;
		if (json_depth(line) > max_depth) return reply_error(client, "request nested too deep");
		const auto request = json::parse(line, nullptr, false);
		if (request.is_discarded() or not request.is_object() or request.size() != 1)
			return reply_error(client, "a request is one JSON object with one key");
		try {
			handle_request(client, request);
		}
		catch (const std::exception& e) {
			reply_error(client, fmt::format("bad request: {}", e.what()));
		}
	}

	void Server::handle_request(Client& client, const json& request) {
		const auto& [name, body] = *request.items().begin();

		if (name == "hello") {
			const int version = body.is_object() ? body.value("v", 0) : 0;
			if (version != Json::schema_version) {
				send(client, Json::dump(json{{"error", "unsupported protocol version"}, {"v", Json::schema_version}}));
				client.closing = true;
				return;
			}
			if (not mirrored.has_value() or subscribers() == 0) mirrored = on_snapshot();
			client.subscribed = true;
			send(client, Json::dump(json{{"v", Json::schema_version}, {"seq", seq}, {"full", *mirrored}, {"update_ms", update_ms}}));
			return;
		}
		if (not client.subscribed) return reply_error(client, "send hello first");

		if (name == "options") {
			if (const auto error = apply_options(body, update_ms); not error.empty()) return reply_error(client, error);
			on_collect(true);
			broadcast(false);
			return;
		}

		if (name == "net") {
			if (not body.is_object()) return reply_error(client, "net must be an object");
			if (const auto it = body.find("select"); it != body.end()) {
				if (not it->is_string() or not Tools::v_contains(Net::interfaces, it->get<string>())) return reply_error(client, "unknown interface");
				Net::selected_iface = it->get<string>();
				Net::rescale = true;
			}
			if (body.value("rescale", false)) Net::rescale = true;
			if (body.value("reset_totals", false) and Net::current_net.contains(Net::selected_iface)) {
				auto& stat = Net::current_net.at(Net::selected_iface).stat;
				if (stat.at("download").offset + stat.at("upload").offset > 0) {
					stat.at("download").offset = 0;
					stat.at("upload").offset = 0;
				}
				else {
					stat.at("download").offset = stat.at("download").last + stat.at("download").rollover;
					stat.at("upload").offset = stat.at("upload").last + stat.at("upload").rollover;
				}
			}
			on_collect(true);
			broadcast(false);
			return;
		}

		if (name == "signal" or name == "nice") {
			const auto result = (name == "signal")
				? Actions::send_signal(body, client.uid, client.pid)
				: Actions::set_nice(body, client.uid, client.pid);
			send(client, Json::dump(json{{"reply", name}, {"ok", result.empty()}, {"error", result}}));
			return;
		}

		if (name == "history") {
			send(client, Json::dump(on_history ? on_history(body)
				: json{{"reply", "history"}, {"ok", false}, {"error", "history is disabled"}}));
			return;
		}

		reply_error(client, fmt::format("unknown request {}", name.substr(0, max_string)));
	}

	void Server::tick() {
		on_collect(false);
		if (subscribers() > 0) broadcast(true);
		else mirrored.reset();
	}

	void Server::broadcast(bool tick) {
		if (subscribers() == 0) {
			mirrored.reset();
			return;
		}
		auto current = on_snapshot();
		auto delta = mirrored.has_value() ? Json::diff(*mirrored, current) : json(json::value_t::discarded);
		mirrored = std::move(current);
		//? A tick is sent even without changes, clients draw one frame per tick
		if (delta.is_discarded() and not tick) return;
		json message = {{"seq", ++seq}, {"tick", tick}, {"update_ms", update_ms}};
		if (not delta.is_discarded()) message["delta"] = std::move(delta);
		const auto line = Json::dump(message);
		for (auto& c : clients) {
			if (c.subscribed and not c.closing) send(c, line);
		}
	}
}
