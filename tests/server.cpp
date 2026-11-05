// SPDX-License-Identifier: Apache-2.0

#include <gtest/gtest.h>

#include "hwlens_actions.hpp"
#include "hwlens_server.hpp"

using Json::json;

TEST(server, json_depth) {
	EXPECT_EQ(Server::json_depth(R"({"a":1})"), 1u);
	EXPECT_EQ(Server::json_depth(R"({"a":[1,{"b":2}]})"), 3u);
	//? Brackets inside strings do not count, escaped quotes do not end a string
	EXPECT_EQ(Server::json_depth(R"({"a":"[[[{{{\"[["})"), 1u);
	EXPECT_EQ(Server::json_depth(std::string(100, '[')), 100u);
}

TEST(server, options_valid_and_atomic) {
	const auto saved = Shared::collect_opts;
	uint64_t update_ms = 1000;
	EXPECT_EQ(Server::apply_options({{"update_ms", 50}, {"freq_mode", "clusters"}, {"show_fans", false}}, update_ms), "");
	EXPECT_EQ(update_ms, Server::min_update_ms);
	EXPECT_EQ(Shared::collect_opts.freq_mode, "clusters");
	EXPECT_FALSE(Shared::collect_opts.show_fans);

	//? One bad field rejects the whole request
	const auto before = Shared::collect_opts.net_iface;
	EXPECT_NE(Server::apply_options({{"net_iface", "eth9"}, {"freq_mode", "turbo"}}, update_ms), "");
	EXPECT_EQ(Shared::collect_opts.net_iface, before);
	EXPECT_NE(Server::apply_options({{"show_fans", "yes"}}, update_ms), "");
	EXPECT_NE(Server::apply_options({{"update_ms", -1}}, update_ms), "");
	EXPECT_NE(Server::apply_options({{"cpu_core_map", "0:1;rm"}}, update_ms), "");
	EXPECT_NE(Server::apply_options({{"disks_filter", "  "}}, update_ms), "");
	EXPECT_NE(Server::apply_options({{"net_iface", std::string(300, 'x')}}, update_ms), "");
	EXPECT_NE(Server::apply_options({{"net_iface", "a\nb"}}, update_ms), "");
	EXPECT_NE(Server::apply_options({{"nope", 1}}, update_ms), "");
	EXPECT_NE(Server::apply_options(json::array(), update_ms), "");
	Shared::collect_opts = saved;
}

TEST(actions, permission_rules) {
	EXPECT_TRUE(Actions::may_act(0, 1000, 1000));
	EXPECT_TRUE(Actions::may_act(1000, 1000, 1000));
	EXPECT_TRUE(Actions::may_act(1000, 0, 1000)); //? saved uid, as the kernel allows
	EXPECT_FALSE(Actions::may_act(1000, 0, 0));
	EXPECT_FALSE(Actions::may_act(1001, 1000, 1000));

	EXPECT_TRUE(Actions::may_renice(1000, 0, 10));
	EXPECT_TRUE(Actions::may_renice(1000, 5, 5));
	EXPECT_FALSE(Actions::may_renice(1000, 5, 0));
	EXPECT_TRUE(Actions::may_renice(0, 5, -10));
	EXPECT_FALSE(Actions::may_renice(0, 0, 20));
	EXPECT_FALSE(Actions::may_renice(0, 0, -21));
}
