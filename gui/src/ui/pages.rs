//! Resource pages. Each takes the latest model and returns the page body.

use std::collections::{HashMap, VecDeque};

use gpui::{Context, Div, Rgba, div, prelude::*, px};

use super::{
    HwlensView, Page,
    fav::Pins,
    widgets::{Graph, MONO, avg, bar, big, chip, overlay, overlay_row, fmt_celsius, fmt_rate, graph_card, graph_panel, segments, card, color, graph_box, last, legend, peak, stat, sub, title},
};
use crate::model::{CPU_FIELDS, Disk, Gpu, Model, Net, Proc, bytes, duration, rate};

const MIN_COL: f32 = 420.;

fn row() -> Div {
    div().flex().flex_wrap().gap_3()
}

/// A column of a wrapping row; its children stretch to the column width.
fn col() -> Div {
    div().flex_1().min_w(px(MIN_COL)).flex().flex_col().gap_3()
}

fn pct(v: f32) -> String {
    format!("{v:.0} %")
}

fn temp_color(t: f32, max: f32) -> Rgba {
    color::level(t, max * 0.75, max * 0.9)
}

// ───────────────────────────── Overview ─────────────────────────────

pub fn overview(m: &Model, cx: &mut Context<HwlensView>, pins: &Pins, favorites: Div) -> Div {
    let link = |page: Page, body: gpui::Stateful<Div>, cx: &mut Context<HwlensView>| {
        div()
            .id(("ov", page as usize))
            .flex_1()
            .min_w(px(240.))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| this.go(page, cx)))
            .child(body.hover(|s| s.border_color(color::muted())))
    };

    let c = &m.cpu;
    let mut cpu_line = Vec::new();
    if let Some(t) = c.temp {
        cpu_line.push(format!("{t:.0} °C"));
    }
    if let Some(w) = c.watts {
        cpu_line.push(format!("{w:.0} W"));
    }
    cpu_line.push(format!("load {:.2} {:.2} {:.2}", c.load_avg[0], c.load_avg[1], c.load_avg[2]));
    let cpu_card = summary("CPU", color::cpu(), pct(last(&c.total)), vec![cpu_line.join(" · ")], Graph::percent().series(&c.total, color::cpu()).name("cpu"));

    let x = &m.mem;
    let mut mem_lines = vec![format!("{} of {} · cached {}", bytes(x.used), bytes(x.total), bytes(x.cached))];
    if x.swap_total > 0 {
        mem_lines.push(format!("swap {} of {}", bytes(x.swap_used), bytes(x.swap_total)));
    }
    let mem_card = summary("Memory", color::mem(), pct(last(&x.used_hist)), mem_lines, Graph::percent().series(&x.used_hist, color::mem()).name("used"));

    let net_card = match m.net() {
        Some(n) => summary(
            "Network",
            color::down(),
            format!("↓ {}", rate(last(&n.down))),
            vec![format!("↑ {} · {} {}", rate(last(&n.up)), n.iface, n.addr)],
            Graph::new().min_max(1024.).fmt(fmt_rate).series(&n.down, color::down()).name("↓").series(&n.up, color::up()).name("↑"),
        ),
        None => summary("Network", color::down(), "—".into(), vec![], Graph::new()),
    };

    let mut cards = row()
        .child(link(Page::Cpu, cpu_card, cx))
        .child(link(Page::Memory, mem_card, cx))
        .child(link(Page::Network, net_card, cx));
    for g in &m.gpus {
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
        let card = summary(
            &format!("GPU · {}", g.name),
            color::gpu(),
            pct(last(&g.util)),
            vec![parts.join(" · "), format!("VRAM {} of {}", bytes(g.mem_used), bytes(g.mem_total))],
            Graph::percent().series(&g.util, color::gpu()).name("gpu"),
        );
        cards = cards.child(link(Page::Gpu, card, cx));
    }

    let mut top_cpu: Vec<&Proc> = m.procs.iter().collect();
    top_cpu.sort_by(|a, b| b.cpu.total_cmp(&a.cpu));
    let mut top_mem: Vec<&Proc> = m.procs.iter().collect();
    top_mem.sort_by(|a, b| b.mem.cmp(&a.mem));

    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(cards)
        .child(row().child(col().child(cores_bars(m))).child(col().child(pressure_card(m))))
        // System and favorites share a row: each takes half the width.
        .child(row().children(m.sys.as_ref().map(|_| col().child(system_card(m, pins)))).child(col().child(favorites)))
        .child(
            row()
                .child(col().child(top_procs("Top CPU", color::cpu(), &top_cpu[..top_cpu.len().min(8)], |p| format!("{:.1} %", p.cpu))))
                .child(col().child(top_procs("Top memory", color::mem(), &top_mem[..top_mem.len().min(8)], |p| bytes(p.mem))))
                .child(col().child(disks_brief(m))),
        )
}

fn system_card(m: &Model, pins: &Pins) -> Div {
    let s = m.sys.as_ref().expect("system card without sys");
    let mut c = card()
        .gap_2()
        .child(title(format!("System · {}", s.hostname), color::text()))
        .child(stat("OS", s.os.clone()))
        .child(stat("Kernel", format!("{} {}", s.kernel, s.arch)))
        .child(pins.row(m, "sys.uptime", "Uptime"))
        .child(pins.row(m, "sys.procs", "Processes"));
    if s.threads.is_some() {
        // procs_running and procs_blocked of /proc/stat count threads, not processes.
        c = c.child(pins.row(m, "sys.threads", "Threads total / running / sleeping"));
    }
    if s.procs_blocked > 0 || s.threads.is_none() {
        c = c.child(pins.row(m, "sys.blocked", "Blocked on I/O (D state)"));
    }
    c.child(pins.row(m, "sys.ctxt", "Context switches"))
        .child(pins.row(m, "sys.intr", "Interrupts"))
        .child(pins.row(m, "sys.forks", "New processes"))
        .child(pins.row(m, "sys.files", "Open files"))
}

/// An overview card: the graph fills it, the numbers sit on top.
fn summary(name: &str, col: Rgba, value: String, lines: Vec<String>, g: Graph) -> gpui::Stateful<Div> {
    graph_card(
        g,
        186.,
        overlay().child(chip(title(name.to_owned(), col))).child(chip(big(value))).children(lines.into_iter().map(|l| chip(sub(l)))),
    )
}

fn cores_bars(m: &Model) -> Div {
    let bars = m.cpu.cores.iter().enumerate().map(|(i, c)| {
        let label = if c.kind == ' ' { format!("{i}") } else { format!("{}{i}", c.kind) };
        let col = if c.kind == 'E' { color::gpu() } else { color::cpu() };
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap_1()
            .w(px(28.))
            .child(
                div()
                    .h(px(52.))
                    .w(px(12.))
                    .rounded_sm()
                    .bg(color::row_alt())
                    .flex()
                    .flex_col()
                    .justify_end()
                    .overflow_hidden()
                    .child(div().w_full().h(px(52. * c.percent.clamp(0., 100.) / 100.)).bg(col)),
            )
            .child(div().text_xs().text_color(color::muted()).child(label))
    });
    card().h_full().child(title(format!("Cores · {}", m.cpu.cores.len()), color::cpu())).child(div().flex().flex_wrap().gap_1().children(bars))
}

fn pressure_card(m: &Model) -> Div {
    let c = &m.cpu;
    let mut body = card().h_full().gap_2().child(title("Pressure & sensors", color::warn()));
    match c.psi {
        Some(psi) => {
            for (name, v) in ["CPU", "Memory", "I/O"].iter().zip(psi) {
                body = body.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(stat(format!("{name} stall (avg10)"), format!("{v:.2} %")))
                        .child(bar(v / 100.0 * 4.0, color::level(v, 5.0, 20.0))),
                );
            }
        }
        None => body = body.child(sub("PSI is not available (kernel without CONFIG_PSI)")),
    }
    if let Some(t) = c.temp {
        body = body.child(stat("CPU package", format!("{t:.0} °C")));
    }
    for f in &c.fans {
        let pwm = f.pwm.map(|p| format!(" · {p:.0} %")).unwrap_or_default();
        body = body.child(stat(format!("Fan {}", f.label), format!("{} rpm{pwm}", f.rpm)));
    }
    if let Some(b) = &c.battery {
        body = body.child(stat("Battery", format!("{:.0} % · {}", b.percent, b.status)));
    }
    body
}

fn disks_brief(m: &Model) -> Div {
    let rows = m.disks.iter().filter(|d| !d.is_swap).map(|d| {
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
            .child(bar(d.used_percent / 100.0, color::level(d.used_percent, 75.0, 90.0)))
            .child(
                div()
                    .flex()
                    .gap_3()
                    .text_xs()
                    .text_color(color::muted())
                    .child(format!("R {}", rate(last(&d.read))))
                    .child(format!("W {}", rate(last(&d.write))))
                    .child(format!("busy {:.0} %", last(&d.activity))),
            )
    });
    card().h_full().gap_2().child(title("Disks", color::disk())).children(rows)
}

fn top_procs(name: &str, col: Rgba, procs: &[&Proc], value: impl Fn(&Proc) -> String) -> Div {
    top_procs_cols(name, col, procs, &[""], |p| vec![value(p)])
}

/// Width of a value column: fits "1023.9 KiB/s" in the monospace font.
const VALUE_W: f32 = 92.;

/// Busiest processes with one or more value columns. Every column has a fixed width, so the
/// rows do not shift when the numbers change length from one tick to the next.
fn top_procs_cols(name: &str, col: Rgba, procs: &[&Proc], headers: &[&str], values: impl Fn(&Proc) -> Vec<String>) -> Div {
    let line = || div().flex().gap_2().text_xs().font_family(MONO);
    let rows = procs.iter().map(|p| {
        line()
            .child(div().w(px(64.)).flex_shrink_0().text_right().text_color(color::faint()).child(p.pid.to_string()))
            .child(div().flex_1().min_w_0().truncate().child(p.name.clone()))
            .child(div().text_color(color::muted()).w(px(80.)).flex_shrink_0().truncate().child(p.user.clone()))
            .children(values(p).into_iter().map(|v| div().w(px(VALUE_W)).flex_shrink_0().text_right().whitespace_nowrap().child(v)))
    });
    let mut head = div().flex().items_center().child(title(name.to_owned(), col)).child(div().flex_1());
    if headers.iter().any(|h| !h.is_empty()) {
        head = head.children(
            headers.iter().map(|h| div().w(px(VALUE_W)).ml_2().flex_shrink_0().text_right().text_xs().text_color(color::muted()).child(h.to_string())),
        );
    }
    card().child(head).children(rows)
}

// ───────────────────────────── CPU ─────────────────────────────

pub fn cpu(m: &Model, pins: &Pins) -> Div {
    let c = &m.cpu;
    let p = c.cores.iter().filter(|k| k.kind == 'P').count();
    let e = c.cores.iter().filter(|k| k.kind == 'E').count();
    let kinds = if p + e > 0 { format!("{p} P + {e} E threads") } else { format!("{} threads", c.cores.len()) };

    let mut g = Graph::percent().stacked().grid();
    let mut legends = div().flex().flex_wrap().gap_2();
    for (i, (name, hist)) in CPU_FIELDS.iter().zip(&c.breakdown).enumerate() {
        if hist.is_empty() {
            continue;
        }
        g = g.series(hist, color::series(i)).name(name);
        legends = legends.child(chip(legend(color::series(i), *name, format!("{:.1} %", last(hist)))));
    }

    let mut info = card()
        .gap_2()
        .child(title("Processor", color::cpu()))
        .child(div().text_lg().child(c.name.clone()))
        .child(pins.row(m, "cpu.freq", "Frequency"))
        .child(stat("Threads", kinds))
        .child(pins.stat("cpu.usage", "Usage now / 1 min avg / peak", format!("{:.0} % / {:.0} % / {:.0} %", last(&c.total), avg(&c.total, 60), peak(&c.total, 60))))
        .child(pins.row(m, "cpu.load", "Load average"));
    if c.psi.is_some() {
        info = info.child(pins.row(m, "cpu.psi", "CPU pressure (some avg10)"));
    }
    if c.watts.is_some() {
        info = info.child(pins.row(m, "cpu.power", "Package power"));
    }
    if let Some(t) = c.temp {
        info = info.child(pins.stat("cpu.temp", "Package temperature", format!("{t:.0} °C (crit {:.0})", c.temp_max)));
    }

    let temp_card = if c.temp_hist.len() >= 2 {
        let g = Graph::new().max(c.temp_max).fmt(fmt_celsius).series(&c.temp_hist, color::temp()).name("package").fit().grid();
        let now = c.temp.map(|t| format!("{t:.0} °C")).unwrap_or_default();
        graph_card(
            g,
            200.,
            overlay()
                .child(overlay_row().child(chip(title("Package temperature", color::temp()))).child(div().flex_1()).child(chip(big(now).text_lg())))
                .child(chip(sub(format!("scale {:.0} °C · last {} samples", c.temp_max, c.temp_hist.len())))),
        )
        .into_any_element()
    } else {
        card().gap_2().child(title("Package temperature", color::temp())).child(sub("no temperature sensor")).into_any_element()
    };

    let cores = c.cores.iter().enumerate().map(|(i, k)| {
        let label = if k.kind == ' ' { format!("CPU {i}") } else { format!("{}{i}", k.kind) };
        let col = if k.kind == 'E' { color::gpu() } else { color::cpu() };
        let t = k.temp.map(|t| format!("{t:.0}°")).unwrap_or_default();
        div().w(px(150.)).child(graph_card(
            Graph::percent().series(&k.hist, col).name("load").fit(),
            70.,
            overlay()
                .child(
                    overlay_row()
                        .text_xs()
                        .child(chip(div().text_color(col).child(label)))
                        .child(div().flex_1())
                        .child(chip(div().text_color(k.temp.map(|t| temp_color(t, c.temp_max)).unwrap_or(color::muted())).child(t))),
                )
                .child(chip(div().text_sm().font_family(MONO).child(format!("{:.0} %", k.percent)))),
        ))
    });

    let mut top: Vec<&Proc> = m.procs.iter().collect();
    top.sort_by(|a, b| b.cpu.total_cmp(&a.cpu));

    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(graph_card(
            g,
            260.,
            overlay()
                .child(overlay_row().child(chip(title("CPU time by category", color::cpu()))).child(div().flex_1()).child(chip(big(pct(last(&c.total))).text_lg())))
                .child(legends),
        ))
        .child(row().child(col().child(info)).child(col().child(temp_card)))
        .child(card().gap_2().child(title(format!("Per core · {}", c.cores.len()), color::cpu())).child(div().flex().flex_wrap().gap_2().children(cores)))
        .child(top_procs("Top CPU", color::cpu(), &top[..top.len().min(12)], |p| format!("{:.1} %", p.cpu)))
}

// ───────────────────────────── Memory ─────────────────────────────

pub fn memory(m: &Model, pins: &Pins) -> Div {
    let x = &m.mem;
    let t = x.total.max(1) as f32;
    let used_f = x.used as f32 / t;
    let cached_f = x.cached as f32 / t;
    let composition = segments(vec![(used_f, color::mem()), (cached_f, color::series(1))], 14.);

    let g = Graph::percent()
        .grid()
        .series(&x.available_hist, color::series(5))
        .name("available")
        .series(&x.cached_hist, color::series(1))
        .name("cached")
        .series(&x.used_hist, color::mem())
        .name("used");

    let mut info = card()
        .gap_2()
        .child(title("Physical memory", color::mem()))
        .child(composition)
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap_3()
                .child(legend(color::mem(), "used", bytes(x.used)))
                .child(legend(color::series(1), "cached", bytes(x.cached)))
                .child(legend(color::row_alt(), "free", bytes(x.free))),
        )
        .child(stat("Total", bytes(x.total)))
        .child(pins.row(m, "mem.used", "Used"))
        .child(pins.row(m, "mem.available", "Available"))
        .child(pins.row(m, "mem.cached", "Cached"));
    if m.cpu.psi.is_some() {
        info = info.child(pins.row(m, "mem.psi", "Memory pressure (some avg10)"));
    }

    let mut swap = card().gap_2().child(title("Swap", color::warn()));
    if x.swap_total > 0 {
        let f = x.swap_used as f32 / x.swap_total as f32;
        swap = swap
            .child(pins.row(m, "mem.swap", "Used"))
            .child(bar(f, color::level(f * 100., 50., 80.)))
            .child(graph_box(Graph::percent().series(&x.swap_hist, color::warn()).name("swap").grid(), 90., None));
    } else {
        swap = swap.child(sub("no swap"));
    }

    let mut top: Vec<&Proc> = m.procs.iter().collect();
    top.sort_by(|a, b| b.mem.cmp(&a.mem));

    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(graph_card(
            g,
            240.,
            overlay()
                .child(overlay_row().child(chip(title("Memory history", color::mem()))).child(div().flex_1()).child(chip(big(pct(last(&x.used_hist))).text_lg())))
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(chip(legend(color::mem(), "used", pct(last(&x.used_hist)))))
                        .child(chip(legend(color::series(1), "cached", pct(last(&x.cached_hist)))))
                        .child(chip(legend(color::series(5), "available", pct(last(&x.available_hist))))),
                ),
        ))
        .child(row().child(col().child(info)).child(col().child(swap)))
        .child(top_procs("Top memory", color::mem(), &top[..top.len().min(12)], |p| bytes(p.mem)))
}

// ───────────────────────────── Disks ─────────────────────────────

pub fn disks(m: &Model, hist: &HashMap<u64, super::procs::ProcHist>, pins: &Pins) -> Div {
    let mut page = row();
    for d in m.disks.iter().filter(|d| !d.is_swap) {
        page = page.child(col().child(disk_card(d, m, pins)));
    }
    // Averages over the last ten seconds: bursty writers would otherwise blink in and out
    // and reorder the list every tick.
    let mut io: Vec<(&Proc, (f32, f32))> = m
        .procs
        .iter()
        .filter_map(|p| Some((p, hist.get(&p.pid)?.io_avg())))
        .filter(|(_, (r, w))| r + w > 0.0)
        .collect();
    io.sort_by(|a, b| (b.1.0 + b.1.1).total_cmp(&(a.1.0 + a.1.1)).then(a.0.pid.cmp(&b.0.pid)));
    io.truncate(12);
    if m.sys.is_some() {
        let avg: HashMap<u64, (f32, f32)> = io.iter().map(|(p, v)| (p.pid, *v)).collect();
        let procs: Vec<&Proc> = io.iter().map(|(p, _)| *p).collect();
        let title_text = format!("Disk I/O by process · {} s average", super::procs::IO_WINDOW);
        let mut top = top_procs_cols(&title_text, color::disk(), &procs, &["read", "write"], |p| {
            let (r, w) = avg.get(&p.pid).copied().unwrap_or_default();
            vec![rate(r), rate(w)]
        });
        if procs.is_empty() {
            top = top.child(sub("no process has read or written in the last seconds"));
        }
        // Room for every row up front, so the card does not grow and shrink with the list.
        page = page.child(col().child(top.min_h(px(330.))));
    }
    if m.disks.iter().all(|d| d.is_swap) {
        page = page.child(sub("hwlensd reports no disks (show_disks is off?)"));
    }
    page
}

fn disk_card(d: &Disk, m: &Model, pins: &Pins) -> Div {
    let io_psi = m.cpu.psi.map(|p| p[2]);
    let key = |what: &str| format!("disk.{}.{what}", d.mount);
    let io = Graph::new().min_max(64. * 1024.).grid().fmt(fmt_rate).series(&d.read, color::down()).name("read").series(&d.write, color::up()).name("write");
    let scale = io.scale(400.);
    let mut head = div()
        .flex()
        .items_center()
        .gap_2()
        .child(title(d.mount.clone(), color::disk()))
        .child(sub(format!("{} · {} · {}", d.name, d.dev, d.fstype)))
        .child(div().flex_1());
    if let Some(t) = d.temp {
        head = head.child(div().text_xs().text_color(temp_color(t, 70.)).child(format!("{t:.0} °C")));
    }
    let mut c = card().gap_2().child(head);
    if !d.is_swap && d.total > 0 {
        c = c
            .child(pins.row(m, &key("used"), "Used"))
            .child(bar(d.used_percent / 100., color::level(d.used_percent, 75., 90.)));
    }
    if !d.read.is_empty() {
        c = c
            .child(graph_panel(
                io,
                120.,
                overlay_row()
                    .child(chip(legend(color::down(), "read", rate(last(&d.read)))))
                    .child(chip(legend(color::up(), "write", rate(last(&d.write)))))
                    .child(div().flex_1())
                    .child(chip(legend(color::disk(), "busy", pct(last(&d.activity)))))
                    .child(chip(sub(rate(scale)))),
            ))
            .child(graph_box(Graph::percent().series(&d.activity, color::disk()).name("busy"), 40., Some("busy 100 %".into())))
            .child(pins.row(m, &key("io"), "Read / write now"))
            .child(stat("Peak (last min) read / write", format!("{} / {}", rate(peak(&d.read, 60)), rate(peak(&d.write, 60)))));
        if d.temp.is_some() {
            c = c.child(pins.row(m, &key("temp"), "Temperature"));
        }
    }
    if io_psi.is_some() && d.mount == "/" {
        c = c.child(pins.row(m, "mem.io_psi", "System I/O pressure (some avg10)"));
    }
    c
}

// ───────────────────────────── Network ─────────────────────────────

pub fn network(m: &Model, pins: &Pins) -> Div {
    let mut nets: Vec<&Net> = m.nets.iter().collect();
    let sel = m.net_selected.as_deref();
    nets.sort_by_key(|n| (Some(n.iface.as_str()) != sel, !n.connected, n.iface == "lo", n.iface.clone()));
    let mut page = row();
    for n in nets {
        page = page.child(col().child(net_card(n, Some(n.iface.as_str()) == sel, m, pins)));
    }
    page
}

fn net_card(n: &Net, selected: bool, m: &Model, pins: &Pins) -> Div {
    let g = Graph::new().min_max(1024.).grid().fmt(fmt_rate).series(&n.down, color::down()).name("↓").series(&n.up, color::up()).name("↑");
    let scale = g.scale(400.);
    let dot = if n.connected { color::mem() } else { color::faint() };
    let mut head = div()
        .flex()
        .items_center()
        .gap_2()
        .child(div().size(px(8.)).rounded_full().bg(dot))
        .child(title(n.iface.clone(), if n.connected { color::down() } else { color::muted() }))
        .child(sub(n.addr.clone()));
    if selected {
        head = head.child(div().text_xs().px_1().rounded_sm().bg(color::selected()).child("selected"));
    }
    let mut c = card()
        .gap_2()
        .child(head)
        .child(graph_panel(
            g,
            120.,
            overlay_row()
                .child(chip(legend(color::down(), "↓", rate(last(&n.down)))))
                .child(chip(legend(color::up(), "↑", rate(last(&n.up)))))
                .child(div().flex_1())
                .child(chip(sub(rate(scale)))),
        ))
        .child(pins.row(m, &format!("net.{}.rate", n.iface), "Now ↓ / ↑"))
        .child(pins.row(m, &format!("net.{}.total", n.iface), "Total ↓ / ↑"))
        .child(stat("Top ↓ / ↑", format!("{} / {}", rate(n.down_top as f32), rate(n.up_top as f32))))
        .child(stat("Avg (last min) ↓ / ↑", format!("{} / {}", rate(avg(&n.down, 60)), rate(avg(&n.up, 60)))));
    if !n.ipv6.is_empty() {
        c = c.child(stat("IPv6", n.ipv6.clone()));
    }
    c
}

// ───────────────────────────── GPU ─────────────────────────────

pub fn gpu(m: &Model, pins: &Pins) -> Div {
    if m.gpus.is_empty() {
        return div().child(sub("No supported GPU (shown_gpus, or hwlensd was built without GPU support)"));
    }
    let mut page = div().flex().flex_col().gap_3();
    for (i, g) in m.gpus.iter().enumerate() {
        page = page.child(gpu_section(i, g, m, pins));
    }
    page
}

fn gpu_section(i: usize, g: &Gpu, m: &Model, pins: &Pins) -> Div {
    let key = |what: &str| format!("gpu.{i}.{what}");
    let opt = |v: Option<u64>, unit: &str| v.map(|x| format!("{x}{unit}")).unwrap_or_else(|| "—".into());
    let small = |name: &str, col: Rgba, now: String, gr: Graph| {
        div().flex_1().min_w(px(260.)).child(graph_card(
            gr.grid(),
            130.,
            overlay()
                .child(overlay_row().child(chip(title(name.to_owned(), col))).child(div().flex_1()).child(chip(div().text_sm().font_family(MONO).child(now)))),
        ))
    };

    let mut stats = card()
        .gap_2()
        .child(title("Details", color::gpu()))
        .child(pins.row(m, &key("load"), "Load"))
        .child(pins.row(m, &key("clock"), "Core / memory clock"))
        .child(pins.row(m, &key("vram"), "VRAM"))
        .child(pins.row(m, &key("power"), "Power"))
        .child(pins.stat(&key("temp"), "Temperature", g.temp.map(|t| format!("{t:.0} °C (max {:.0})", g.temp_max)).unwrap_or_else(|| "—".into())))
        .child(pins.row(m, &key("fan"), "Fan"))
        .child(stat("Performance state", g.pstate.map(|p| format!("P{p}")).unwrap_or_else(|| "—".into())))
        .child(stat("Encoder / decoder", format!("{} / {}", opt(g.encoder, " %"), opt(g.decoder, " %"))))
        .child(stat(
            "PCIe RX / TX",
            format!(
                "{} / {}",
                g.pcie_rx.map(|b| rate(b as f32)).unwrap_or_else(|| "—".into()),
                g.pcie_tx.map(|b| rate(b as f32)).unwrap_or_else(|| "—".into())
            ),
        ));
    stats = stats.child(stat("Clock limits", if g.throttle.is_empty() { "none".to_owned() } else { g.throttle.join(", ") }));

    let mut vram_procs: Vec<&Proc> = m.procs.iter().filter(|p| p.vram.is_some_and(|v| v > 0)).collect();
    vram_procs.sort_by(|a, b| b.vram.cmp(&a.vram));

    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(div().text_lg().font_weight(gpui::FontWeight::SEMIBOLD).child(g.name.clone()))
        .child(
            row()
                .child(small("Utilization", color::gpu(), pct(last(&g.util)), Graph::percent().series(&g.util, color::gpu()).name("load")))
                .child(small("VRAM", color::mem(), pct(last(&g.vram_hist)), Graph::percent().series(&g.vram_hist, color::mem()).name("vram")))
                .child(small("Power", color::warn(), g.power.map(|p| format!("{p:.0} W")).unwrap_or_default(), Graph::percent().series(&g.power_hist, color::warn()).name("% of limit")))
                .child(small("Memory controller", color::series(1), pct(last(&g.mem_ctrl_hist)), Graph::percent().series(&g.mem_ctrl_hist, color::series(1)).name("busy")))
                .child(small(
                    "Temperature",
                    color::temp(),
                    g.temp.map(|t| format!("{t:.0} °C")).unwrap_or_default(),
                    Graph::new().max(g.temp_max).fmt(fmt_celsius).series(&g.temp_hist, color::temp()).name("gpu").fit(),
                )),
        )
        .child(
            row()
                .child(col().child(stats))
                .child(col().child(top_procs("VRAM by process", color::gpu(), &vram_procs[..vram_procs.len().min(12)], |p| bytes(p.vram.unwrap_or(0))))),
        )
}

// ───────────────────────────── Sensors ─────────────────────────────

/// Cards spread over `n` columns, each going to the shortest one so far (by `weight`, about
/// its number of rows). A wrapping row would give every card of a line the height of the tallest.
fn masonry(cards: Vec<(usize, Div)>, n: usize) -> Div {
    let mut cols: Vec<(usize, Vec<Div>)> = (0..n.max(1)).map(|_| (0, Vec::new())).collect();
    for (weight, card) in cards {
        let shortest = cols.iter_mut().min_by_key(|(h, _)| *h).expect("a column");
        shortest.0 += weight + 2; // title and padding
        shortest.1.push(card);
    }
    row().children(cols.into_iter().filter(|(_, c)| !c.is_empty()).map(|(_, c)| col().children(c)))
}

fn kind_title(kind: &str) -> &'static str {
    match kind {
        "temp" => "temperatures",
        "fan" => "fans",
        "pwm" => "fan duty",
        "voltage" => "voltages",
        "current" => "currents",
        "power" => "power",
        "energy" => "energy",
        _ => "other",
    }
}

pub fn sensors(m: &Model, hist: &HashMap<String, VecDeque<f32>>, pins: &Pins) -> Div {
    let c = &m.cpu;
    let temp_row = |name: String, t: f32, max: f32| {
        div()
            .flex()
            .items_center()
            .gap_2()
            .text_xs()
            .child(div().w(px(150.)).flex_shrink_0().truncate().text_color(color::muted()).child(name))
            .child(div().flex_1().child(bar(t / max, temp_color(t, max))))
            .child(div().w(px(56.)).flex_shrink_0().text_right().font_family(MONO).child(format!("{t:.0} °C")))
    };

    // Package as a bar, cores as a compact grid of tiles.
    let mut cpu_temps = card().gap_2().child(title("CPU temperatures", color::temp()));
    if let Some(t) = c.temp {
        cpu_temps = cpu_temps.child(temp_row("Package".into(), t, c.temp_max));
    }
    let tiles = c.cores.iter().enumerate().filter_map(|(i, k)| {
        let t = k.temp?;
        let label = if k.kind == ' ' { format!("CPU {i}") } else { format!("{}{i}", k.kind) };
        Some(
            div()
                .w(px(76.))
                .flex()
                .flex_col()
                .gap(px(2.))
                .text_xs()
                .child(
                    div()
                        .flex()
                        .child(div().text_color(color::muted()).child(label))
                        .child(div().flex_1())
                        .child(div().font_family(MONO).text_color(temp_color(t, c.temp_max)).child(format!("{t:.0}°"))),
                )
                .child(segments(vec![(t / c.temp_max, temp_color(t, c.temp_max))], 4.)),
        )
    });
    cpu_temps = cpu_temps.child(div().flex().flex_wrap().gap_x_3().gap_y_2().children(tiles));

    let mut other = card().gap_2().child(title("Other temperatures", color::temp()));
    let mut other_n = 0;
    for d in m.disks.iter().filter(|d| d.temp.is_some()) {
        other = other.child(temp_row(format!("Disk {} ({})", d.mount, d.dev), d.temp.unwrap_or(0.), 80.));
        other_n += 1;
    }
    for g in &m.gpus {
        if let Some(t) = g.temp {
            other = other.child(temp_row(format!("GPU {}", g.name), t, g.temp_max));
            other_n += 1;
        }
    }

    let mut fans = card().gap_2().child(title("Fans", color::down()));
    let mut fans_n = 0;
    if c.fans.is_empty() {
        fans = fans.child(sub("no fans reported"));
    }
    for f in &c.fans {
        fans = fans
            .child(stat(f.label.clone(), format!("{} rpm{}", f.rpm, f.pwm.map(|p| format!(" · PWM {p:.0} %")).unwrap_or_default())))
            .child(bar(f.pwm.unwrap_or(0.) / 100., color::down()));
        fans_n += 2;
    }
    for g in &m.gpus {
        if let Some(f) = g.fan {
            fans = fans.child(stat(format!("GPU {}", g.name), format!("{f} %"))).child(bar(f as f32 / 100., color::gpu()));
            fans_n += 2;
        }
    }

    let mut power = card().gap_2().child(title("Power", color::warn()));
    let mut power_n = 0;
    if let Some(w) = c.watts {
        power = power.child(stat("CPU package", format!("{w:.1} W")));
        power_n += 1;
    }
    for g in &m.gpus {
        if let Some(p) = g.power {
            power = power.child(stat(format!("GPU {}", g.name), format!("{p:.1} W")));
            power_n += 1;
        }
    }
    if let Some(b) = &c.battery {
        power = power
            .child(stat("Battery", format!("{:.0} % · {}", b.percent, b.status)))
            .child(stat("Battery power", format!("{:.1} W", b.watts)));
        power_n += 2;
        if b.seconds > 0 {
            power = power.child(stat("Time left", duration(b.seconds)));
        }
    }

    let mut summary = vec![(1 + c.cores.len().div_ceil(5), cpu_temps)];
    if other_n > 0 {
        summary.push((other_n, other));
    }
    summary.push((fans_n.max(1), fans));
    if power_n > 0 {
        summary.push((power_n, power));
    }
    let top = masonry(summary, 2);

    if m.hwmon.is_empty() {
        // Older daemons only name the sensors.
        let names = card()
            .gap_1()
            .child(title(format!("Detected sensors · {}", c.sensor_names.len()), color::muted()))
            .children(c.sensor_names.iter().map(|n| div().text_xs().font_family(MONO).text_color(color::muted()).child(n.clone())));
        return div().flex().flex_col().gap_3().child(top).child(names);
    }

    // One card per chip and kind, so a big Super I/O chip does not make one endless card.
    let mut groups: Vec<((&str, &str), Vec<&crate::model::HwSensor>)> = Vec::new();
    for s in &m.hwmon {
        let key = (s.chip.as_str(), s.kind.as_str());
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, list)) => list.push(s),
            None => groups.push((key, vec![s])),
        }
    }
    let mut cards = Vec::new();
    let mut inactive_total = 0;
    for ((chip, kind), list) in groups {
        // Unconnected inputs read 0 the whole time: name them in one line instead of a row each.
        let dead = |s: &crate::model::HwSensor| {
            matches!(kind, "temp" | "fan") && s.value == 0.0 && hist.get(&s.key()).is_none_or(|h| h.iter().all(|v| *v == 0.0))
        };
        let (idle, live): (Vec<_>, Vec<_>) = list.into_iter().partition(|s| dead(s));
        inactive_total += idle.len();
        let rows = live.iter().map(|s| {
            let col = match s.kind.as_str() {
                "temp" => s.crit.or(s.max).map(|lim| temp_color(s.value, lim)).unwrap_or(color::temp()),
                "fan" | "pwm" => color::down(),
                "power" => color::warn(),
                "voltage" | "current" => color::disk(),
                _ => color::muted(),
            };
            let h: Vec<f32> = hist.get(&s.key()).map(|h| h.iter().copied().collect()).unwrap_or_default();
            let mut g = Graph::new().series(&h, col).fit().min_max(match s.kind.as_str() {
                "temp" => 10.,
                "fan" => 500.,
                "pwm" => 100.,
                _ => 0.1,
            });
            if s.kind == "pwm" {
                g = g.max(100.);
            }
            let limit = s.crit.or(s.max).map(|l| format!(" / {}", s.format(l))).unwrap_or_default();
            div()
                .group("pin")
                .flex()
                .items_center()
                .gap_2()
                .text_xs()
                .child(pins.toggle(format!("hwmon.{}", s.key())))
                .child(div().w(px(130.)).flex_shrink_0().truncate().text_color(color::muted()).child(s.label.clone()))
                .child(div().flex_1().min_w(px(40.)).child(graph_box(g, 16., None)))
                .child(div().w(px(130.)).flex_shrink_0().text_right().font_family(MONO).child(format!("{}{limit}", s.format(s.value))))
        });
        let mut card = card().gap_1().child(
            div()
                .flex()
                .gap_2()
                .child(title(chip.to_owned(), color::temp()))
                .child(div().text_xs().text_color(color::muted()).child(kind_title(kind))),
        );
        card = card.children(rows);
        if !idle.is_empty() {
            let names: Vec<&str> = idle.iter().map(|s| s.label.as_str()).collect();
            card = card.child(div().text_xs().text_color(color::faint()).child(format!("inactive (reads 0): {}", names.join(", "))));
        }
        cards.push((live.len() + idle.len().div_ceil(6), card));
    }
    let inactive = if inactive_total > 0 { format!(" · {inactive_total} inactive") } else { String::new() };
    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(top)
        .child(title(format!("All hwmon sensors · {}{inactive}", m.hwmon.len()), color::muted()))
        .child(masonry(cards, 2))
}
