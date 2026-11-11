//! The window: summary cards with graphs, per-core load, disks and a process table.

use std::sync::Arc;

use futures::{StreamExt, channel::mpsc::UnboundedReceiver};
use gpui::{
    App, Context, Div, FocusHandle, KeyDownEvent, PathBuilder, Rgba, ScrollStrategy, SharedString,
    Stateful, Task, UniformListScrollHandle, Window, canvas, div, point, prelude::*, px, uniform_list,
};

use crate::{
    client::{Event, Requests},
    model::{Model, Proc, bytes, rate},
};

mod color {
    use gpui::{Rgba, rgb};
    pub fn bg() -> Rgba { rgb(0x0f1115) }
    pub fn card() -> Rgba { rgb(0x171a21) }
    pub fn border() -> Rgba { rgb(0x262b36) }
    pub fn row_alt() -> Rgba { rgb(0x1a1e26) }
    pub fn selected() -> Rgba { rgb(0x23324d) }
    pub fn text() -> Rgba { rgb(0xe6e8ee) }
    pub fn muted() -> Rgba { rgb(0x8a92a6) }
    pub fn cpu() -> Rgba { rgb(0x4f9dff) }
    pub fn mem() -> Rgba { rgb(0x3ccf91) }
    pub fn down() -> Rgba { rgb(0x5ec4ff) }
    pub fn up() -> Rgba { rgb(0xff8a5c) }
    pub fn gpu() -> Rgba { rgb(0xb48cff) }
    pub fn warn() -> Rgba { rgb(0xffb547) }
    pub fn bad() -> Rgba { rgb(0xff5c6c) }
}

const MONO: &str = "DejaVu Sans Mono";

#[derive(Clone, Copy, PartialEq)]
enum SortKey {
    Pid,
    Name,
    User,
    Cpu,
    Mem,
    Threads,
}

/// Column title, width in px (None: takes the rest), sort key, right aligned.
const COLUMNS: [(&str, Option<f32>, Option<SortKey>, bool); 8] = [
    ("PID", Some(72.), Some(SortKey::Pid), true),
    ("Name", Some(180.), Some(SortKey::Name), false),
    ("User", Some(110.), Some(SortKey::User), false),
    ("CPU %", Some(72.), Some(SortKey::Cpu), true),
    ("Memory", Some(96.), Some(SortKey::Mem), true),
    ("Thr", Some(52.), Some(SortKey::Threads), true),
    ("S", Some(28.), None, false),
    ("Command", None, None, false),
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
    rows: Vec<Proc>,
    sort: SortKey,
    descending: bool,
    filter: String,
    selected: Option<u64>,
    focus: FocusHandle,
    scroll: UniformListScrollHandle,
    _events: Task<()>,
}

impl HwlensView {
    pub fn new(requests: Requests, mut rx: UnboundedReceiver<Event>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let events = cx.spawn(async move |this, cx| {
            while let Some(ev) = rx.next().await {
                if this.update(cx, |view, cx| view.on_event(ev, cx)).is_err() {
                    break;
                }
            }
        });
        let focus = cx.focus_handle();
        window.focus(&focus);
        HwlensView {
            model: None,
            link: Link::Connecting,
            message: None,
            requests,
            rows: Vec::new(),
            sort: SortKey::Cpu,
            descending: true,
            filter: String::new(),
            selected: None,
            focus,
            scroll: UniformListScrollHandle::new(),
            _events: events,
        }
    }

    fn on_event(&mut self, ev: Event, cx: &mut Context<Self>) {
        match ev {
            Event::Model(m) => {
                self.model = Some(m);
                self.link = Link::Up;
                self.rebuild_rows();
            }
            Event::Disconnected(why) => self.link = Link::Down(why),
            Event::Reply { ok, text } => self.message = Some((ok, text)),
        }
        cx.notify();
    }

    fn rebuild_rows(&mut self) {
        let Some(m) = &self.model else { return };
        let needle = self.filter.to_lowercase();
        let mut rows: Vec<Proc> = m
            .procs
            .iter()
            .filter(|p| {
                needle.is_empty()
                    || p.name.to_lowercase().contains(&needle)
                    || p.cmd.to_lowercase().contains(&needle)
                    || p.user.to_lowercase().contains(&needle)
                    || p.pid.to_string() == needle
            })
            .cloned()
            .collect();
        let key = self.sort;
        rows.sort_by(|a, b| {
            let ord = match key {
                SortKey::Pid => a.pid.cmp(&b.pid),
                SortKey::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                SortKey::User => a.user.cmp(&b.user),
                SortKey::Cpu => a.cpu.total_cmp(&b.cpu),
                SortKey::Mem => a.mem.cmp(&b.mem),
                SortKey::Threads => a.threads.cmp(&b.threads),
            };
            // Ties keep a stable order by pid, so rows do not jump between ticks.
            let ord = if self.descending { ord.reverse() } else { ord };
            ord.then(a.pid.cmp(&b.pid))
        });
        self.rows = rows;
    }

    fn set_sort(&mut self, key: SortKey, cx: &mut Context<Self>) {
        if self.sort == key {
            self.descending = !self.descending;
        } else {
            self.sort = key;
            // Text columns read best A→Z, numbers biggest first.
            self.descending = !matches!(key, SortKey::Name | SortKey::User | SortKey::Pid);
        }
        self.rebuild_rows();
        cx.notify();
    }

    fn selected_index(&self) -> Option<usize> {
        let pid = self.selected?;
        self.rows.iter().position(|p| p.pid == pid)
    }

    fn move_selection(&mut self, by: isize) {
        if self.rows.is_empty() {
            return;
        }
        let ix = match self.selected_index() {
            Some(i) => (i as isize + by).clamp(0, self.rows.len() as isize - 1) as usize,
            None => 0,
        };
        self.selected = Some(self.rows[ix].pid);
        self.scroll.scroll_to_item(ix, ScrollStrategy::Center);
    }

    fn signal_selected(&mut self, signal: i32) {
        let Some(pid) = self.selected else { return };
        let name = self.rows.iter().find(|p| p.pid == pid).map(|p| p.name.clone()).unwrap_or_default();
        self.message = Some(match self.requests.signal(pid, signal) {
            Ok(()) => (true, format!("sent {} to {name} ({pid})", if signal == 9 { "SIGKILL" } else { "SIGTERM" })),
            Err(e) => (false, format!("{e:#}")),
        });
    }

    fn on_key(&mut self, ev: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let ks = &ev.keystroke;
        let m = ks.modifiers;
        match ks.key.as_str() {
            "q" if m.control => cx.quit(),
            "escape" if !self.filter.is_empty() => {
                self.filter.clear();
                self.rebuild_rows();
            }
            "escape" => self.selected = None,
            "backspace" => {
                self.filter.pop();
                self.rebuild_rows();
            }
            "up" => self.move_selection(-1),
            "down" => self.move_selection(1),
            "pageup" => self.move_selection(-20),
            "pagedown" => self.move_selection(20),
            "delete" => self.signal_selected(if m.shift { 9 } else { 15 }),
            _ => {
                let Some(ch) = ks.key_char.as_deref() else { return };
                if m.control || m.alt || m.platform || ch.chars().any(char::is_control) {
                    return;
                }
                self.filter.push_str(ch);
                self.rebuild_rows();
            }
        }
        cx.notify();
    }
}

impl Render for HwlensView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let root = div()
            .key_context("hwlens")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .size_full()
            .flex()
            .flex_col()
            .gap_3()
            .p_3()
            .bg(color::bg())
            .text_color(color::text())
            .font_family("Noto Sans")
            .text_sm();

        let Some(m) = self.model.clone() else {
            let text = match &self.link {
                Link::Down(why) => why.clone(),
                _ => "Connecting to hwlensd…".to_owned(),
            };
            return root.items_center().justify_center().child(div().text_color(color::muted()).child(text));
        };

        root.child(self.header(&m))
            .child(
                div()
                    .flex()
                    .gap_3()
                    .h(px(186.))
                    .child(cpu_card(&m))
                    .child(mem_card(&m))
                    .child(net_card(&m))
                    .children(m.gpus.first().map(gpu_card)),
            )
            .child(div().flex().gap_3().child(cores_card(&m)).child(disks_card(&m)))
            .child(self.proc_card(cx))
            .child(self.footer())
    }
}

impl HwlensView {
    fn header(&self, m: &Model) -> Div {
        let (dot, status) = match &self.link {
            Link::Up => (color::mem(), format!("hwlensd · {} ms", m.update_ms)),
            Link::Connecting => (color::warn(), "connecting".to_owned()),
            Link::Down(why) => (color::bad(), why.clone()),
        };
        div()
            .flex()
            .items_center()
            .gap_3()
            .child(div().text_lg().font_weight(gpui::FontWeight::BOLD).child("hwlens"))
            .child(div().text_color(color::muted()).child(format!("{} · {}", m.cpu.name, m.cpu.hz)))
            .child(div().flex_1())
            .child(div().size(px(8.)).rounded_full().bg(dot))
            .child(div().text_color(color::muted()).text_xs().max_w(px(520.)).truncate().child(status))
    }

    fn proc_card(&self, cx: &mut Context<Self>) -> Div {
        let filter = if self.filter.is_empty() {
            div().text_color(color::muted()).child("type to filter")
        } else {
            div().child(format!("filter: {}▏", self.filter))
        };
        let head = div()
            .flex()
            .items_center()
            .gap_3()
            .child(title("Processes", color::text()))
            .child(div().text_color(color::muted()).child(format!("{} shown", self.rows.len())))
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
            .children(COLUMNS.iter().enumerate().map(|(i, &(name, width, key, right))| {
                let arrow = match key {
                    Some(k) if k == self.sort => if self.descending { " ▼" } else { " ▲" },
                    _ => "",
                };
                let cell = cell(width, right).id(("col", i)).child(format!("{name}{arrow}"));
                match key {
                    Some(k) => cell
                        .cursor_pointer()
                        .hover(|s| s.text_color(color::text()))
                        .on_click(cx.listener(move |this, _, _, cx| this.set_sort(k, cx))),
                    None => cell,
                }
            }));

        let list = uniform_list(
            "procs",
            self.rows.len(),
            cx.processor(|this, range: std::ops::Range<usize>, _window, cx| {
                range.map(|ix| this.row(ix, cx)).collect::<Vec<_>>()
            }),
        )
        .track_scroll(self.scroll.clone())
        .flex_1();

        card().flex_1().min_h(px(200.)).child(head).child(columns).child(list)
    }

    fn row(&self, ix: usize, cx: &mut Context<Self>) -> Stateful<Div> {
        let p = &self.rows[ix];
        let pid = p.pid;
        let bg = if self.selected == Some(pid) {
            color::selected()
        } else if ix % 2 == 1 {
            color::row_alt()
        } else {
            color::card()
        };
        let cpu_color = if p.cpu >= 50.0 { color::warn() } else { color::text() };
        let values: [(SharedString, Option<Rgba>); 8] = [
            (pid.to_string().into(), Some(color::muted())),
            (p.name.clone().into(), None),
            (p.user.clone().into(), Some(color::muted())),
            (format!("{:.1}", p.cpu).into(), Some(cpu_color)),
            (if p.mem == 0 { "—".into() } else { bytes(p.mem).into() }, None),
            (p.threads.to_string().into(), Some(color::muted())),
            (p.state.to_string().into(), Some(color::muted())),
            (p.cmd.clone().into(), Some(color::muted())),
        ];
        div()
            .id(("proc", pid as usize))
            .w_full()
            .flex()
            .px_2()
            .py(px(2.))
            .bg(bg)
            .text_xs()
            .font_family(MONO)
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                this.selected = Some(pid);
                cx.notify();
            }))
            .children(COLUMNS.iter().zip(values).map(|(&(_, width, _, right), (text, col))| {
                let c = cell(width, right).child(text);
                match col {
                    Some(col) => c.text_color(col),
                    None => c,
                }
            }))
    }

    fn footer(&self) -> Div {
        let msg = self.message.as_ref().map(|(ok, text)| {
            div().text_color(if *ok { color::mem() } else { color::bad() }).child(text.clone())
        });
        div()
            .flex()
            .gap_3()
            .text_xs()
            .text_color(color::muted())
            .child("↑↓ select · click a column to sort · Del SIGTERM · Shift+Del SIGKILL · Esc clear · Ctrl+Q quit")
            .child(div().flex_1())
            .children(msg)
    }
}

fn card() -> Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .p_3()
        .rounded_lg()
        .bg(color::card())
        .border_1()
        .border_color(color::border())
        .overflow_hidden()
}

fn title(text: &str, col: Rgba) -> Div {
    div().text_xs().font_weight(gpui::FontWeight::SEMIBOLD).text_color(col).child(text.to_uppercase())
}

fn cell(width: Option<f32>, right: bool) -> Div {
    let c = div().px_1().whitespace_nowrap().overflow_hidden().truncate();
    let c = match width {
        Some(w) => c.w(px(w)).flex_shrink_0(),
        None => c.flex_1().min_w_0(),
    };
    if right { c.text_right() } else { c }
}

fn big(text: String) -> Div {
    div().text_2xl().font_weight(gpui::FontWeight::SEMIBOLD).child(text)
}

fn sub(text: String) -> Div {
    div().text_xs().text_color(color::muted()).truncate().child(text)
}

fn summary_card(name: &str, col: Rgba, value: String, lines: Vec<String>, graph: impl IntoElement) -> Div {
    card()
        .flex_1()
        .min_w_0()
        .child(title(name, col))
        .child(big(value))
        .children(lines.into_iter().map(sub))
        .child(div().flex_1().min_h(px(36.)).mt_1().child(graph))
}

fn cpu_card(m: &Model) -> Div {
    let c = &m.cpu;
    let mut extra = Vec::new();
    let mut parts = Vec::new();
    if let Some(t) = c.temp {
        parts.push(format!("{t:.0} °C"));
    }
    if let Some(w) = c.watts {
        parts.push(format!("{w:.0} W"));
    }
    parts.push(format!("load {:.2} {:.2} {:.2}", c.load_avg[0], c.load_avg[1], c.load_avg[2]));
    extra.push(parts.join(" · "));
    let now = c.total.last().copied().unwrap_or(0.0);
    summary_card("CPU", color::cpu(), format!("{now:.0} %"), extra, graph(vec![(c.total.clone(), color::cpu())], 100.0))
}

fn mem_card(m: &Model) -> Div {
    let x = &m.mem;
    let pct = if x.total > 0 { x.used as f64 * 100.0 / x.total as f64 } else { 0.0 };
    let mut lines = vec![format!("{} of {} · cached {}", bytes(x.used), bytes(x.total), bytes(x.cached))];
    if x.swap_total > 0 {
        lines.push(format!("swap {} of {}", bytes(x.swap_used), bytes(x.swap_total)));
    }
    summary_card("Memory", color::mem(), format!("{pct:.0} %"), lines, graph(vec![(x.used_hist.clone(), color::mem())], 100.0))
}

fn net_card(m: &Model) -> Div {
    let Some(n) = &m.net else {
        return summary_card("Network", color::down(), "—".into(), vec![], div());
    };
    let down = n.down.last().copied().unwrap_or(0.0);
    let up = n.up.last().copied().unwrap_or(0.0);
    let window = |v: &[f32]| v.iter().rev().take(300).fold(0.0f32, |a, &b| a.max(b));
    let max = window(&n.down).max(window(&n.up)).max(1024.0);
    summary_card(
        "Network",
        color::down(),
        format!("↓ {}", rate(down)),
        vec![format!("↑ {} · {} {}", rate(up), n.iface, n.ip)],
        graph(vec![(n.down.clone(), color::down()), (n.up.clone(), color::up())], max),
    )
}

fn gpu_card(g: &crate::model::Gpu) -> Div {
    let now = g.util.last().copied().unwrap_or(0.0);
    let mut parts = Vec::new();
    if let Some(t) = g.temp {
        parts.push(format!("{t:.0} °C"));
    }
    if let Some(p) = g.power {
        parts.push(format!("{p:.0} W"));
    }
    if let Some(c) = g.clock_mhz {
        parts.push(format!("{c} MHz"));
    }
    let mut lines = vec![parts.join(" · ")];
    if g.mem_total > 0 {
        lines.push(format!("VRAM {} of {}", bytes(g.mem_used), bytes(g.mem_total)));
    }
    card()
        .flex_1()
        .min_w_0()
        .child(title(&format!("GPU · {}", g.name), color::gpu()))
        .child(big(format!("{now:.0} %")))
        .children(lines.into_iter().map(sub))
        .child(div().flex_1().min_h(px(36.)).mt_1().child(graph(vec![(g.util.clone(), color::gpu())], 100.0)))
}

fn cores_card(m: &Model) -> Div {
    let bars = m.cpu.cores.iter().enumerate().map(|(i, c)| {
        let label = if c.kind == ' ' { format!("{i}") } else { format!("{}{i}", c.kind) };
        let col = if c.kind == 'E' { color::gpu() } else { color::cpu() };
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap_1()
            .w(px(30.))
            .child(
                div()
                    .h(px(46.))
                    .w(px(12.))
                    .rounded_sm()
                    .bg(color::row_alt())
                    .flex()
                    .flex_col()
                    .justify_end()
                    .overflow_hidden()
                    .child(div().w_full().h(px(46. * c.percent.clamp(0., 100.) / 100.)).bg(col)),
            )
            .child(div().text_xs().text_color(color::muted()).child(label))
    });
    card()
        .flex_1()
        .min_w_0()
        .child(title(&format!("Cores · {}", m.cpu.cores.len()), color::cpu()))
        .child(div().flex().flex_wrap().gap_1().children(bars))
}

fn disks_card(m: &Model) -> Div {
    let rows = m.disks.iter().map(|d| {
        let col = if d.used_percent >= 90.0 { color::bad() } else if d.used_percent >= 75.0 { color::warn() } else { color::mem() };
        div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(
                div()
                    .flex()
                    .text_xs()
                    .child(div().flex_1().truncate().child(format!("{} ({})", d.mount, d.fstype)))
                    .child(div().text_color(color::muted()).child(format!("{} / {}", bytes(d.used), bytes(d.total)))),
            )
            .child(
                div()
                    .h(px(6.))
                    .w_full()
                    .rounded_full()
                    .bg(color::row_alt())
                    .child(div().h_full().rounded_full().w(gpui::relative(d.used_percent.clamp(0., 100.) / 100.)).bg(col)),
            )
    });
    card().w(px(380.)).flex_shrink_0().child(title("Disks", color::mem())).child(div().flex().flex_col().gap_2().children(rows))
}

/// Area graphs drawn right to left, one sample every 2 px, all series on one scale.
fn graph(series: Vec<(Vec<f32>, Rgba)>, max: f32) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, (), window, _: &mut App| {
            const STEP: f32 = 2.0;
            let w = f32::from(bounds.size.width);
            let h = f32::from(bounds.size.height);
            let max = max.max(f32::EPSILON);
            for (values, col) in &series {
                let n = ((w / STEP) as usize + 1).min(values.len());
                if n < 2 {
                    continue;
                }
                let vals = &values[values.len() - n..];
                let x0 = bounds.right() - px(STEP * (n - 1) as f32);
                let at = |i: usize, v: f32| {
                    point(x0 + px(STEP * i as f32), bounds.top() + px(h - (v / max).clamp(0.0, 1.0) * h))
                };

                let mut area = PathBuilder::fill();
                area.move_to(point(x0, bounds.bottom()));
                for (i, &v) in vals.iter().enumerate() {
                    area.line_to(at(i, v));
                }
                area.line_to(bounds.bottom_right());
                area.close();
                if let Ok(path) = area.build() {
                    let mut fill = *col;
                    fill.a = 0.16;
                    window.paint_path(path, fill);
                }

                let mut line = PathBuilder::stroke(px(1.5));
                line.move_to(at(0, vals[0]));
                for (i, &v) in vals.iter().enumerate().skip(1) {
                    line.line_to(at(i, v));
                }
                if let Ok(path) = line.build() {
                    window.paint_path(path, *col);
                }
            }
        },
    )
    .size_full()
}
