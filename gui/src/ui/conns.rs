//! Connections page: TCP state summary and every socket with its process.

use std::collections::HashMap;

use gpui::{Context, Div, Rgba, Stateful, div, prelude::*, px, uniform_list};

use super::{
    HwlensView,
    widgets::{MONO, card, color, legend, sub, title},
};
use crate::model::{Model, Socket};

#[derive(Clone)]
pub struct ConnRow {
    pub sock: Socket,
    pub name: String,
}

/// Rows of the sockets table for a filter (protocol, state, address, port, pid or process name).
pub fn rows(m: &Model, filter: &str) -> Vec<ConnRow> {
    let Some(so) = &m.sockets else { return Vec::new() };
    let names: HashMap<u64, &str> = m.procs.iter().map(|p| (p.pid, p.name.as_str())).collect();
    let needle = filter.to_lowercase();
    so.list
        .iter()
        .map(|s| ConnRow { sock: s.clone(), name: names.get(&s.pid).copied().unwrap_or_default().to_owned() })
        .filter(|r| {
            needle.is_empty()
                || r.name.to_lowercase().contains(&needle)
                || r.sock.local.contains(&needle)
                || r.sock.remote.contains(&needle)
                || r.sock.state.to_lowercase().contains(&needle)
                || r.sock.proto.contains(&needle)
                || r.sock.pid.to_string() == needle
        })
        .collect()
}

fn state_color(state: &str) -> Rgba {
    match state {
        "LISTEN" | "UNCONN" => color::cpu(),
        "ESTABLISHED" => color::mem(),
        "TIME_WAIT" | "CLOSE_WAIT" | "FIN_WAIT1" | "FIN_WAIT2" | "LAST_ACK" | "CLOSING" => color::warn(),
        "SYN_SENT" | "SYN_RECV" => color::disk(),
        _ => color::muted(),
    }
}

/// Column title, width (None: the rest).
const COLUMNS: [(&str, Option<f32>); 6] =
    [("Proto", Some(56.)), ("State", Some(110.)), ("Local", Some(260.)), ("Remote", None), ("PID", Some(80.)), ("Process", Some(180.))];

fn cell(width: Option<f32>) -> Div {
    let c = div().px_1().whitespace_nowrap().overflow_hidden().truncate();
    match width {
        Some(w) => c.w(px(w)).flex_shrink_0(),
        None => c.flex_1().min_w_0(),
    }
}

impl HwlensView {
    pub(super) fn conns_page(&mut self, m: &Model, cx: &mut Context<Self>) -> Div {
        let Some(so) = &m.sockets else {
            return div().child(sub("This hwlensd does not report sockets; update the daemon."));
        };

        let mut states = div().flex().flex_wrap().gap_3();
        for (state, n) in &so.tcp_states {
            states = states.child(legend(state_color(state), state.clone(), n.to_string()));
        }
        states = states.child(legend(color::series(5), "UDP", so.udp.to_string()));

        let mut per_proc: HashMap<u64, usize> = HashMap::new();
        for s in so.list.iter().filter(|s| s.pid != 0) {
            *per_proc.entry(s.pid).or_default() += 1;
        }
        let mut busiest: Vec<(u64, usize)> = per_proc.into_iter().collect();
        busiest.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let names: HashMap<u64, &str> = m.procs.iter().map(|p| (p.pid, p.name.as_str())).collect();
        let top = busiest.iter().take(6).map(|(pid, n)| {
            div()
                .flex()
                .gap_2()
                .text_xs()
                .font_family(MONO)
                .child(div().w(px(70.)).text_right().text_color(color::faint()).child(pid.to_string()))
                .child(div().flex_1().truncate().child(names.get(pid).copied().unwrap_or("?").to_owned()))
                .child(div().child(format!("{n} sockets")))
        });

        let filter = if self.conn_filter.is_empty() {
            div().text_color(color::faint()).child("type to filter: port, address, state, process")
        } else {
            div().px_2().rounded_md().bg(color::selected()).child(format!("filter: {}▏", self.conn_filter))
        };
        let unknown = so.list.iter().filter(|s| s.pid == 0).count();
        let head = div()
            .flex()
            .items_center()
            .gap_2()
            .child(title("Sockets", color::text()))
            .child(div().text_xs().text_color(color::muted()).child(format!(
                "{} shown · {} total{}",
                self.conn_rows.len(),
                so.total,
                if so.total as usize > so.list.len() { " · time-wait not listed" } else { "" }
            )))
            .child(div().flex_1())
            .child(filter);
        let columns = div()
            .flex()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(color::border())
            .text_xs()
            .text_color(color::muted())
            .children(COLUMNS.iter().map(|(name, w)| cell(*w).child(*name)));
        let list = uniform_list(
            "conns",
            self.conn_rows.len(),
            cx.processor(|this, range: std::ops::Range<usize>, _window, _cx| range.map(|ix| this.conn_row(ix)).collect::<Vec<_>>()),
        )
        .flex_1();

        div()
            .flex()
            .flex_col()
            .gap_3()
            .size_full()
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(card().flex_1().gap_2().child(title("TCP states", color::down())).child(states).when(unknown > 0, |d| {
                        d.child(sub(format!("{unknown} sockets without a known owner (other users; the daemon needs CAP_SYS_PTRACE)")))
                    }))
                    .child(card().flex_1().gap_1().child(title("Most sockets", color::down())).children(top)),
            )
            .child(card().flex_1().min_h(px(200.)).child(head).child(columns).child(list))
    }

    fn conn_row(&self, ix: usize) -> Stateful<Div> {
        let r = &self.conn_rows[ix];
        let s = &r.sock;
        let values: [(String, Rgba); 6] = [
            (s.proto.clone(), color::muted()),
            (s.state.clone(), state_color(&s.state)),
            (s.local.clone(), color::text()),
            (if s.remote.ends_with(":0") && (s.state == "LISTEN" || s.state == "UNCONN") { String::new() } else { s.remote.clone() }, color::text()),
            (if s.pid == 0 { "—".into() } else { s.pid.to_string() }, color::faint()),
            (r.name.clone(), color::text()),
        ];
        div()
            .id(("conn", ix))
            .w_full()
            .flex()
            .px_2()
            .py(px(2.))
            .bg(if ix % 2 == 1 { color::row_alt() } else { color::card() })
            .text_xs()
            .font_family(MONO)
            .children(COLUMNS.iter().zip(values).map(|((_, w), (text, col))| cell(*w).text_color(col).child(text)))
    }
}
