//! The window: a sidebar with live values and one page at a time.

mod conns;
mod debug;
mod fav;
mod history;
mod pages;
mod procs;
mod svc;
mod widgets;

use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

use futures::{StreamExt, channel::mpsc::UnboundedReceiver};
use gpui::{Context, Div, FocusHandle, KeyDownEvent, ScrollStrategy, Task, UniformListScrollHandle, Window, div, prelude::*, px};

use crate::{
    client::{Event, Requests},
    model::{Model, rate},
    services::{Services, Update},
};
use procs::ProcState;
use widgets::{color, last};

impl Page {
    pub fn from_name(name: &str) -> Option<Page> {
        PAGES.iter().find(|(_, _, n)| n.eq_ignore_ascii_case(name)).map(|(p, _, _)| *p)
    }

    pub fn names() -> Vec<&'static str> {
        PAGES.iter().map(|(_, _, n)| *n).collect()
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Page {
    Overview,
    Cpu,
    Memory,
    Disks,
    Network,
    Gpu,
    Processes,
    Connections,
    Sensors,
    Services,
    History,
    Debug,
}

const PAGES: [(Page, &str, &str); 12] = [
    (Page::Overview, "◎", "Overview"),
    (Page::Cpu, "▦", "CPU"),
    (Page::Memory, "▤", "Memory"),
    (Page::Disks, "◫", "Disks"),
    (Page::Network, "⇅", "Network"),
    (Page::Gpu, "◈", "GPU"),
    (Page::Processes, "≡", "Processes"),
    (Page::Connections, "⇄", "Connections"),
    (Page::Sensors, "♨", "Sensors"),
    (Page::Services, "⚙", "Services"),
    (Page::History, "◷", "History"),
    (Page::Debug, "⚑", "Debug"),
];

enum Link {
    Connecting,
    Up,
    Down(String),
}

pub struct HwlensView {
    model: Option<Arc<Model>>,
    link: Link,
    message: Option<(bool, String)>,
    requests: Requests,
    page: Page,
    procs: ProcState,
    conn_filter: String,
    conn_rows: Vec<conns::ConnRow>,
    /// Client-side history of every hwmon sensor, by HwSensor::key().
    sensor_hist: HashMap<String, VecDeque<f32>>,
    history: history::HistState,
    services: Services,
    svc: svc::SvcState,
    svc_scroll: UniformListScrollHandle,
    /// Favorite keys in display order (fav.rs), saved in ~/.config/hwlens/gui.json.
    favorites: Vec<String>,
    debug: debug::DebugState,
    focus: FocusHandle,
    scroll: UniformListScrollHandle,
    _events: Task<()>,
}

impl HwlensView {
    pub fn new(requests: Requests, services: Services, page: Page, mut rx: UnboundedReceiver<Event>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let events = cx.spawn(async move |this, cx| {
            while let Some(ev) = rx.next().await {
                if this.update(cx, |view, cx| view.on_event(ev, cx)).is_err() {
                    break;
                }
            }
        });
        let focus = cx.focus_handle();
        window.focus(&focus);
        let favorites = fav::load();
        let polls_services = page == Page::Services || (page == Page::Overview && favorites.iter().any(|k| k.starts_with("svc.")));
        services.set_active(polls_services);
        HwlensView {
            model: None,
            link: Link::Connecting,
            message: None,
            requests,
            page,
            procs: ProcState::default(),
            conn_filter: String::new(),
            conn_rows: Vec::new(),
            sensor_hist: HashMap::new(),
            history: history::HistState::default(),
            services,
            svc: svc::SvcState::default(),
            svc_scroll: UniformListScrollHandle::new(),
            favorites,
            debug: debug::DebugState::default(),
            focus,
            scroll: UniformListScrollHandle::new(),
            _events: events,
        }
    }

    fn on_event(&mut self, ev: Event, cx: &mut Context<Self>) {
        match ev {
            Event::Model(m) => {
                self.debug.models += 1;
                if m.tick && self.debug.models % debug::SAMPLE_TICKS == 1 {
                    self.debug.sample();
                }
                if m.tick {
                    self.procs.record(&m);
                    self.record_sensors(&m);
                }
                self.procs.rebuild(&m);
                self.conn_rows = conns::rows(&m, &self.conn_filter);
                self.model = Some(m);
                self.link = Link::Up;
                let stale = self.history.requested.is_none_or(|t| t.elapsed().as_secs() >= history::REFRESH_SECS);
                if self.page == Page::History && stale {
                    self.history_request();
                }
            }
            Event::Disconnected(why) => {
                if matches!(self.link, Link::Up) {
                    self.debug.reconnects += 1;
                }
                self.link = Link::Down(why);
                self.history.requested = None;
            }
            Event::Reply { ok, text } => self.message = Some((ok, text)),
            Event::History(v) => self.history_reply(v),
            Event::Services(Update::Units(units)) => {
                self.svc.units = units;
                self.svc.loaded = true;
                self.svc.error = None;
                self.svc.rebuild();
            }
            Event::Services(Update::Error(e)) => self.svc.error = Some(e),
            Event::Services(Update::Journal { scope, unit, text }) => self.svc.journal = Some((scope, unit, text)),
        }
        cx.notify();
    }

    /// The services thread polls for the Services page, and for service favorites on the overview.
    fn update_services_polling(&self) {
        let fav = self.page == Page::Overview && self.favorites.iter().any(|k| k.starts_with("svc."));
        self.services.set_active(self.page == Page::Services || fav);
    }

    fn go(&mut self, page: Page, cx: &mut Context<Self>) {
        if page != self.page {
            self.debug.page_switches += 1;
        }
        self.page = page;
        self.update_services_polling();
        if page == Page::History && self.model.is_some() {
            self.history_request();
        }
        cx.notify();
    }

    fn rebuild(&mut self, cx: &mut Context<Self>) {
        if let Some(m) = self.model.clone() {
            self.procs.rebuild(&m);
            self.conn_rows = conns::rows(&m, &self.conn_filter);
        }
        self.svc.rebuild();
        cx.notify();
    }

    fn record_sensors(&mut self, m: &Model) {
        let keys: std::collections::HashSet<String> = m.hwmon.iter().map(|s| s.key()).collect();
        self.sensor_hist.retain(|k, _| keys.contains(k));
        for s in &m.hwmon {
            let h = self.sensor_hist.entry(s.key()).or_default();
            h.push_back(s.value);
            if h.len() > procs::HIST {
                h.pop_front();
            }
        }
    }

    /// The text filter that typing edits on the current page.
    fn filter_mut(&mut self) -> &mut String {
        match self.page {
            Page::Connections => &mut self.conn_filter,
            Page::Services => &mut self.svc.filter,
            _ => &mut self.procs.filter,
        }
    }

    fn proc_name(&self, pid: u64) -> String {
        self.model
            .as_ref()
            .and_then(|m| m.procs.iter().find(|p| p.pid == pid))
            .map(|p| p.name.clone())
            .unwrap_or_default()
    }

    fn send_signal(&mut self, pid: u64, signal: i32) {
        let name = self.proc_name(pid);
        self.message = Some(match self.requests.signal(pid, signal) {
            Ok(()) => (true, format!("sent signal {signal} to {name} ({pid})")),
            Err(e) => (false, format!("{e:#}")),
        });
    }

    fn send_nice(&mut self, pid: u64, nice: i64) {
        let name = self.proc_name(pid);
        self.message = Some(match self.requests.nice(pid, nice) {
            Ok(()) => (true, format!("asked nice {nice} for {name} ({pid})")),
            Err(e) => (false, format!("{e:#}")),
        });
    }

    fn move_selection(&mut self, by: isize) {
        let rows = &self.procs.rows;
        if rows.is_empty() {
            return;
        }
        let ix = match self.procs.selected_index() {
            Some(i) => (i as isize + by).clamp(0, rows.len() as isize - 1) as usize,
            None => 0,
        };
        self.procs.selected = Some(rows[ix].proc.pid);
        self.scroll.scroll_to_item(ix, ScrollStrategy::Center);
    }

    fn move_svc_selection(&mut self, by: isize) {
        let n = self.svc.rows.len();
        if n == 0 {
            return;
        }
        let ix = match self.svc.selected_index() {
            Some(i) => (i as isize + by).clamp(0, n as isize - 1) as usize,
            None => 0,
        };
        let u = &self.svc.units[self.svc.rows[ix]];
        self.svc_select((u.scope, u.name.clone()));
        self.svc_scroll.scroll_to_item(ix, ScrollStrategy::Center);
    }

    fn on_key(&mut self, ev: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let ks = &ev.keystroke;
        let m = ks.modifiers;
        if m.control {
            match ks.key.as_str() {
                "q" => cx.quit(),
                "t" => {
                    self.procs.tree = !self.procs.tree;
                    self.page = Page::Processes;
                    self.rebuild(cx);
                }
                k => {
                    // Ctrl+1…9 and Ctrl+0 for the tenth page.
                    let i = k.parse::<usize>().ok().map(|i| if i == 0 { 10 } else { i });
                    if let Some(i) = i.filter(|i| (1..=PAGES.len()).contains(i)) {
                        self.go(PAGES[i - 1].0, cx);
                    }
                }
            }
            return;
        }
        let on_procs = self.page == Page::Processes;
        let on_svc = self.page == Page::Services;
        let filtering = on_procs || on_svc || self.page == Page::Connections;
        match ks.key.as_str() {
            "escape" if filtering && !self.filter_mut().is_empty() => {
                self.filter_mut().clear();
                self.rebuild(cx);
            }
            "escape" if on_svc => self.svc.selected = None,
            "escape" => self.procs.selected = None,
            "backspace" if filtering => {
                self.filter_mut().pop();
                self.rebuild(cx);
            }
            "up" if on_procs => self.move_selection(-1),
            "down" if on_procs => self.move_selection(1),
            "pageup" if on_procs => self.move_selection(-20),
            "pagedown" if on_procs => self.move_selection(20),
            "up" if on_svc => self.move_svc_selection(-1),
            "down" if on_svc => self.move_svc_selection(1),
            "pageup" if on_svc => self.move_svc_selection(-20),
            "pagedown" if on_svc => self.move_svc_selection(20),
            "delete" if on_procs => {
                if let Some(pid) = self.procs.selected {
                    self.send_signal(pid, if m.shift { 9 } else { 15 });
                }
            }
            _ => {
                let Some(ch) = ks.key_char.as_deref() else { return };
                if m.alt || m.platform || ch.chars().any(char::is_control) {
                    return;
                }
                // Typing searches connections and services on their pages, processes everywhere else.
                if !matches!(self.page, Page::Connections | Page::Services) {
                    self.page = Page::Processes;
                }
                self.filter_mut().push_str(ch);
                self.rebuild(cx);
            }
        }
        cx.notify();
    }

    fn sidebar(&self, m: Option<&Model>, cx: &mut Context<Self>) -> Div {
        let value = |page: Page| -> String {
            let Some(m) = m else { return String::new() };
            match page {
                Page::Cpu => format!("{:.0}%", last(&m.cpu.total)),
                Page::Memory => format!("{:.0}%", last(&m.mem.used_hist)),
                Page::Disks => {
                    let io: f32 = m.disks.iter().filter(|d| !d.is_swap).map(|d| last(&d.read) + last(&d.write)).sum();
                    if io > 0.0 { rate(io) } else { String::new() }
                }
                Page::Network => m.net().map(|n| rate(last(&n.down) + last(&n.up))).unwrap_or_default(),
                Page::Gpu => m.gpus.first().map(|g| format!("{:.0}%", last(&g.util))).unwrap_or_default(),
                Page::Processes => m.procs.len().to_string(),
                Page::Connections => m.sockets.as_ref().map(|s| s.total.to_string()).unwrap_or_default(),
                Page::Sensors => m.cpu.temp.map(|t| format!("{t:.0}°")).unwrap_or_default(),
                Page::Services => {
                    let failed = self.svc.units.iter().filter(|u| u.active == "failed").count();
                    if failed > 0 { format!("{failed} ✕") } else { String::new() }
                }
                Page::Overview | Page::History => String::new(),
                Page::Debug => self.debug.last.as_ref().map(|r| format!("{:.0} M", r.rss_kb as f32 / 1024.)).unwrap_or_default(),
            }
        };
        let items = PAGES.iter().enumerate().map(|(i, &(page, icon, name))| {
            let active = self.page == page;
            div()
                .id(("nav", i))
                .flex()
                .items_center()
                .gap_2()
                .px_2()
                .py_1()
                .rounded_md()
                .cursor_pointer()
                .when(active, |d| d.bg(color::selected()))
                .when(!active, |d| d.hover(|s| s.bg(color::hover())))
                .text_color(if active { color::text() } else { color::muted() })
                .on_click(cx.listener(move |this, _, _, cx| this.go(page, cx)))
                .child(div().w(px(16.)).child(icon))
                .child(div().flex_1().child(name))
                .child(div().text_xs().font_family(widgets::MONO).text_color(color::faint()).child(value(page)))
        });
        let (dot, status) = match &self.link {
            Link::Up => (color::mem(), m.map(|m| format!("hwlensd · {} ms", m.update_ms)).unwrap_or_default()),
            Link::Connecting => (color::warn(), "connecting".to_owned()),
            Link::Down(_) => (color::bad(), "disconnected".to_owned()),
        };
        div()
            .w(px(196.))
            .flex_shrink_0()
            .h_full()
            .flex()
            .flex_col()
            .gap_1()
            .p_2()
            .bg(color::sidebar())
            .border_r_1()
            .border_color(color::border())
            .child(div().px_2().py_2().text_lg().font_weight(gpui::FontWeight::BOLD).child("hwlens"))
            // One line whatever the clock: the name is cut, the frequency has a fixed width
            // ("800 MHz" is wider than "4.9 GHz" and used to wrap, moving the menu).
            .children(m.map(|m| {
                div()
                    .flex()
                    .gap_1()
                    .px_2()
                    .pb_2()
                    .text_xs()
                    .text_color(color::muted())
                    .whitespace_nowrap()
                    .child(div().flex_1().min_w_0().truncate().child(short_cpu_name(&m.cpu.name).to_owned()))
                    .child(div().flex_shrink_0().w(px(58.)).text_right().font_family(widgets::MONO).child(m.cpu.hz.clone()))
            }))
            .children(items)
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .text_xs()
                    .text_color(color::muted())
                    .child(div().size(px(8.)).rounded_full().bg(dot))
                    .child(status),
            )
            .child(div().px_2().text_xs().text_color(color::faint()).child("Ctrl+1…0 pages · type to search"))
    }
}

/// "12th Gen i9-12900K" -> "i9-12900K": the generation does not fit next to the clock.
fn short_cpu_name(name: &str) -> &str {
    match name.split_once(" Gen ") {
        Some((generation, rest)) if generation.ends_with(|c: char| c.is_ascii_alphabetic()) && generation.starts_with(|c: char| c.is_ascii_digit()) => rest,
        _ => name,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn short_cpu_name_drops_the_generation() {
        assert_eq!(super::short_cpu_name("12th Gen i9-12900K"), "i9-12900K");
        assert_eq!(super::short_cpu_name("AMD Ryzen 9 7950X"), "AMD Ryzen 9 7950X");
    }
}

impl Render for HwlensView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.debug.frames += 1;
        let size = window.viewport_size();
        self.debug.saw_size(f32::from(size.width), f32::from(size.height));
        let m = self.model.clone();
        let pins = self.pins(cx);
        let root = div()
            .key_context("hwlens")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .size_full()
            .flex()
            .bg(color::bg())
            .text_color(color::text())
            .font_family("Noto Sans")
            .text_sm()
            .child(self.sidebar(m.as_deref(), cx));

        let Some(m) = m else {
            let text = match &self.link {
                Link::Down(why) => why.clone(),
                _ => "Connecting to hwlensd…".to_owned(),
            };
            return root.child(div().flex_1().flex().items_center().justify_center().text_color(color::muted()).child(text));
        };

        let body = match self.page {
            Page::Processes => div().flex_1().min_h(px(0.)).p_3().child(self.procs_page(&m, cx)),
            Page::Connections => div().flex_1().min_h(px(0.)).p_3().child(self.conns_page(&m, cx)),
            Page::Services => div().flex_1().min_h(px(0.)).p_3().child(self.svc_page(cx)),
            page => {
                let content = match page {
                    Page::Overview => {
                        let favs = self.favorites_card(&m, cx);
                        pages::overview(&m, cx, &pins, favs)
                    }
                    Page::Cpu => pages::cpu(&m, &pins),
                    Page::Memory => pages::memory(&m, &pins),
                    Page::Disks => pages::disks(&m, &self.procs.hist, &pins),
                    Page::Network => pages::network(&m, &pins),
                    Page::Gpu => pages::gpu(&m, &pins),
                    Page::Sensors => pages::sensors(&m, &self.sensor_hist, &pins),
                    Page::History => self.history_page(cx),
                    Page::Debug => self.debug_page(cx),
                    Page::Processes | Page::Connections | Page::Services => unreachable!(),
                };
                div().flex_1().min_h(px(0.)).child(div().id(("page", page as usize)).size_full().overflow_y_scroll().p_3().child(content))
            }
        };

        let mut status = div().flex().gap_3().px_3().py_1().border_t_1().border_color(color::border()).text_xs().text_color(color::muted());
        status = match &self.link {
            Link::Down(why) => status.child(div().text_color(color::bad()).truncate().child(why.clone())),
            _ => status.child(
                "Processes: ↑↓ select · Del SIGTERM · Shift+Del SIGKILL · Ctrl+T tree · Esc clear · Ctrl+Q quit",
            ),
        };
        status = status.child(div().flex_1());
        if let Some((ok, text)) = &self.message {
            status = status.child(div().text_color(if *ok { color::mem() } else { color::bad() }).child(text.clone()));
        }

        root.child(div().flex_1().min_w_0().h_full().flex().flex_col().child(body).child(status))
    }
}
