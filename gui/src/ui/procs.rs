//! Processes page: flat or tree table, details of the selected process, actions.

use std::collections::{HashMap, HashSet, VecDeque};

use gpui::{Context, Div, Rgba, SharedString, Stateful, div, prelude::*, px, uniform_list};

use super::{
    HwlensView,
    widgets::{Graph, MONO, big, card, color, graph_box, stat, sub, title},
};
use crate::model::{Model, Proc, bytes};

/// Samples of client-side history kept per process.
pub const HIST: usize = 300;

#[derive(Default)]
pub struct ProcHist {
    pub cpu: VecDeque<f32>,
    pub mem: VecDeque<f32>,
    pub io: VecDeque<f32>,
}

#[derive(Clone, Copy, PartialEq)]
pub enum SortKey {
    Pid,
    Name,
    User,
    Cpu,
    Mem,
    Vram,
    Io,
    Threads,
    Nice,
}

struct Column {
    name: &'static str,
    width: Option<f32>,
    key: Option<SortKey>,
    right: bool,
}

const fn colm(name: &'static str, width: Option<f32>, key: Option<SortKey>, right: bool) -> Column {
    Column { name, width, key, right }
}

const COLUMNS: [Column; 11] = [
    colm("PID", Some(76.), Some(SortKey::Pid), true),
    colm("Name", Some(220.), Some(SortKey::Name), false),
    colm("User", Some(96.), Some(SortKey::User), false),
    colm("CPU %", Some(64.), Some(SortKey::Cpu), true),
    colm("Memory", Some(88.), Some(SortKey::Mem), true),
    colm("VRAM", Some(80.), Some(SortKey::Vram), true),
    colm("Disk I/O", Some(88.), Some(SortKey::Io), true),
    colm("Thr", Some(44.), Some(SortKey::Threads), true),
    colm("NI", Some(36.), Some(SortKey::Nice), true),
    colm("S", Some(24.), None, false),
    colm("Command", None, None, false),
];

#[derive(Clone)]
pub struct Row {
    pub proc: Proc,
    pub depth: usize,
    pub has_children: bool,
}

pub struct ProcState {
    pub rows: Vec<Row>,
    pub sort: SortKey,
    pub descending: bool,
    pub filter: String,
    pub selected: Option<u64>,
    pub tree: bool,
    pub hide_kernel: bool,
    pub collapsed: HashSet<u64>,
    pub hist: HashMap<u64, ProcHist>,
}

impl Default for ProcState {
    fn default() -> Self {
        ProcState {
            rows: Vec::new(),
            sort: SortKey::Cpu,
            descending: true,
            filter: String::new(),
            selected: None,
            tree: false,
            hide_kernel: false,
            collapsed: HashSet::new(),
            hist: HashMap::new(),
        }
    }
}

fn is_kernel(p: &Proc) -> bool {
    p.pid == 2 || p.ppid == 2
}

impl ProcState {
    /// Adds one sample per process; forgets processes that exited.
    pub fn record(&mut self, m: &Model) {
        let alive: HashSet<u64> = m.procs.iter().map(|p| p.pid).collect();
        self.hist.retain(|pid, _| alive.contains(pid));
        for p in &m.procs {
            let h = self.hist.entry(p.pid).or_default();
            h.cpu.push_back(p.cpu);
            h.mem.push_back(p.mem as f32);
            h.io.push_back(p.io_read + p.io_write);
            if h.cpu.len() > HIST {
                h.cpu.pop_front();
                h.mem.pop_front();
                h.io.pop_front();
            }
        }
    }

    fn cmp(&self, a: &Proc, b: &Proc) -> std::cmp::Ordering {
        let ord = match self.sort {
            SortKey::Pid => a.pid.cmp(&b.pid),
            SortKey::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortKey::User => a.user.cmp(&b.user),
            SortKey::Cpu => a.cpu.total_cmp(&b.cpu),
            SortKey::Mem => a.mem.cmp(&b.mem),
            SortKey::Vram => a.vram.unwrap_or(0).cmp(&b.vram.unwrap_or(0)),
            SortKey::Io => (a.io_read + a.io_write).total_cmp(&(b.io_read + b.io_write)),
            SortKey::Threads => a.threads.cmp(&b.threads),
            SortKey::Nice => a.nice.cmp(&b.nice),
        };
        // Ties keep a stable order by pid, so rows do not jump between ticks.
        let ord = if self.descending { ord.reverse() } else { ord };
        ord.then(a.pid.cmp(&b.pid))
    }

    pub fn rebuild(&mut self, m: &Model) {
        let needle = self.filter.to_lowercase();
        let matches = |p: &Proc| {
            needle.is_empty()
                || p.name.to_lowercase().contains(&needle)
                || p.cmd.to_lowercase().contains(&needle)
                || p.user.to_lowercase().contains(&needle)
                || p.unit.to_lowercase().contains(&needle)
                || p.pid.to_string() == needle
        };
        let procs: Vec<&Proc> = m.procs.iter().filter(|p| !(self.hide_kernel && is_kernel(p))).collect();

        if !self.tree {
            let mut list: Vec<&Proc> = procs.into_iter().filter(|p| matches(p)).collect();
            list.sort_by(|a, b| self.cmp(a, b));
            self.rows = list.into_iter().map(|p| Row { proc: p.clone(), depth: 0, has_children: false }).collect();
            return;
        }

        let by_pid: HashMap<u64, &Proc> = procs.iter().map(|p| (p.pid, *p)).collect();
        // With a filter, show the matches and the chain of parents above them.
        let mut keep: HashSet<u64> = HashSet::new();
        for p in procs.iter().filter(|p| matches(p)) {
            let mut cur = Some(*p);
            while let Some(c) = cur {
                if !keep.insert(c.pid) {
                    break;
                }
                cur = by_pid.get(&c.ppid).copied();
            }
        }
        let mut children: HashMap<u64, Vec<&Proc>> = HashMap::new();
        let mut roots = Vec::new();
        for p in procs.iter().filter(|p| keep.contains(&p.pid)) {
            if p.ppid != p.pid && keep.contains(&p.ppid) {
                children.entry(p.ppid).or_default().push(p);
            } else {
                roots.push(*p);
            }
        }
        for v in children.values_mut() {
            v.sort_by(|a, b| self.cmp(a, b));
        }
        roots.sort_by(|a, b| self.cmp(a, b));

        let mut rows = Vec::with_capacity(keep.len());
        let mut stack: Vec<(&Proc, usize)> = roots.into_iter().rev().map(|p| (p, 0)).collect();
        while let Some((p, depth)) = stack.pop() {
            let kids = children.get(&p.pid);
            rows.push(Row { proc: p.clone(), depth, has_children: kids.is_some() });
            if let Some(kids) = kids
                && (!self.collapsed.contains(&p.pid) || !needle.is_empty())
            {
                stack.extend(kids.iter().rev().map(|k| (*k, depth + 1)));
            }
        }
        self.rows = rows;
    }

    pub fn set_sort(&mut self, key: SortKey) {
        if self.sort == key {
            self.descending = !self.descending;
        } else {
            self.sort = key;
            // Text columns read best A→Z, numbers biggest first.
            self.descending = !matches!(key, SortKey::Name | SortKey::User | SortKey::Pid | SortKey::Nice);
        }
    }

    pub fn selected_index(&self) -> Option<usize> {
        let pid = self.selected?;
        self.rows.iter().position(|r| r.proc.pid == pid)
    }
}

fn cell(width: Option<f32>, right: bool) -> Div {
    let c = div().px_1().whitespace_nowrap().overflow_hidden().truncate();
    let c = match width {
        Some(w) => c.w(px(w)).flex_shrink_0(),
        None => c.flex_1().min_w_0(),
    };
    if right { c.text_right() } else { c }
}

pub(super) fn button(id: &'static str, label: &str, col: Rgba) -> Stateful<Div> {
    div()
        .id(id)
        .px_2()
        .py(px(3.))
        .rounded_md()
        .border_1()
        .border_color(color::border())
        .text_xs()
        .text_color(col)
        .cursor_pointer()
        .hover(|s| s.bg(color::hover()))
        .child(label.to_owned())
}

pub fn state_name(s: char) -> &'static str {
    match s {
        'R' => "running",
        'S' => "sleeping",
        'D' => "disk sleep",
        'Z' => "zombie",
        'T' => "stopped",
        't' => "traced",
        'I' => "idle",
        'X' => "dead",
        _ => "unknown",
    }
}

impl HwlensView {
    pub(super) fn procs_page(&mut self, m: &Model, cx: &mut Context<Self>) -> Div {
        let st = &self.procs;
        let filter = if st.filter.is_empty() {
            div().text_color(color::faint()).child("type to filter")
        } else {
            div().px_2().rounded_md().bg(color::selected()).child(format!("filter: {}▏", st.filter))
        };
        let toggle = |id: &'static str, label: &str, on: bool| {
            button(id, label, if on { color::text() } else { color::muted() }).when(on, |b| b.bg(color::selected()))
        };
        let head = div()
            .flex()
            .items_center()
            .gap_2()
            .child(title("Processes", color::text()))
            .child(div().text_xs().text_color(color::muted()).child(format!("{} shown of {}", st.rows.len(), m.procs.len())))
            .child(div().flex_1())
            .child(filter)
            .child(toggle("tree", "Tree (Ctrl+T)", st.tree).on_click(cx.listener(|this, _, _, cx| {
                this.procs.tree = !this.procs.tree;
                this.rebuild(cx);
            })))
            .child(toggle("kern", "Hide kernel threads", st.hide_kernel).on_click(cx.listener(|this, _, _, cx| {
                this.procs.hide_kernel = !this.procs.hide_kernel;
                this.rebuild(cx);
            })));

        let columns = div()
            .flex()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(color::border())
            .text_xs()
            .text_color(color::muted())
            .children(COLUMNS.iter().enumerate().map(|(i, c)| {
                let arrow = match c.key {
                    Some(k) if k == st.sort => if st.descending { " ▼" } else { " ▲" },
                    _ => "",
                };
                let el = cell(c.width, c.right).id(("col", i)).child(format!("{}{arrow}", c.name));
                match c.key {
                    Some(k) => el.cursor_pointer().hover(|s| s.text_color(color::text())).on_click(cx.listener(move |this, _, _, cx| {
                        this.procs.set_sort(k);
                        this.rebuild(cx);
                    })),
                    None => el,
                }
            }));

        let list = uniform_list(
            "procs",
            st.rows.len(),
            cx.processor(|this, range: std::ops::Range<usize>, _window, cx| range.map(|ix| this.proc_row(ix, cx)).collect::<Vec<_>>()),
        )
        .track_scroll(self.scroll.clone())
        .flex_1();

        let table = card().flex_1().min_w_0().h_full().child(head).child(columns).child(list);
        let mut page = div().flex().gap_3().size_full().child(table);
        if let Some(pid) = self.procs.selected {
            page = page.child(self.details(pid, m, cx));
        }
        page
    }

    fn proc_row(&self, ix: usize, cx: &mut Context<Self>) -> Stateful<Div> {
        let r = &self.procs.rows[ix];
        let p = &r.proc;
        let pid = p.pid;
        let bg = if self.procs.selected == Some(pid) {
            color::selected()
        } else if ix % 2 == 1 {
            color::row_alt()
        } else {
            color::card()
        };
        let cpu_color = if p.cpu >= 50.0 { color::warn() } else { color::text() };

        let mut name = div().flex().items_center().min_w_0();
        if self.procs.tree {
            name = name.child(div().flex_shrink_0().w(px(12. * r.depth as f32)));
            let arrow = if !r.has_children { "  " } else if self.procs.collapsed.contains(&pid) { "▸ " } else { "▾ " };
            name = name.child(
                div()
                    .id(("fold", pid as usize))
                    .flex_shrink_0()
                    .text_color(color::muted())
                    .when(r.has_children, |d| {
                        d.cursor_pointer().on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            if !this.procs.collapsed.remove(&pid) {
                                this.procs.collapsed.insert(pid);
                            }
                            this.rebuild(cx);
                        }))
                    })
                    .child(arrow),
            );
        }
        name = name.child(div().truncate().child(p.name.clone()));

        let io = p.io_read + p.io_write;
        let values: [(Div, Option<Rgba>); 11] = [
            (div().child(pid.to_string()), Some(color::faint())),
            (name, None),
            (div().child(p.user.clone()), Some(color::muted())),
            (div().child(format!("{:.1}", p.cpu)), Some(cpu_color)),
            (div().child(if p.mem == 0 { "—".to_owned() } else { bytes(p.mem) }), None),
            (div().child(p.vram.map(bytes).unwrap_or_default()), Some(color::gpu())),
            (div().child(if io > 0.0 { crate::model::rate(io) } else { String::new() }), Some(color::disk())),
            (div().child(p.threads.to_string()), Some(color::muted())),
            (div().child(p.nice.to_string()), Some(if p.nice != 0 { color::warn() } else { color::faint() })),
            (div().child(p.state.to_string()), Some(if p.state == 'R' { color::mem() } else { color::muted() })),
            (div().child(p.cmd.clone()), Some(color::muted())),
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
                this.procs.selected = Some(pid);
                cx.notify();
            }))
            .children(COLUMNS.iter().zip(values).map(|(c, (content, col))| {
                let el = cell(c.width, c.right).child(content);
                match col {
                    Some(col) => el.text_color(col),
                    None => el,
                }
            }))
    }

    fn details(&self, pid: u64, m: &Model, cx: &mut Context<Self>) -> Div {
        let panel = card().w(px(360.)).flex_shrink_0().h_full().gap_2();
        let Some(p) = m.procs.iter().find(|p| p.pid == pid) else {
            return panel.child(title("Process", color::text())).child(sub(format!("{pid} has exited")));
        };
        let parent = m.procs.iter().find(|q| q.pid == p.ppid).map(|q| format!("{} ({})", q.name, q.pid)).unwrap_or_else(|| p.ppid.to_string());
        let children = m.procs.iter().filter(|q| q.ppid == pid).count();
        let hist = self.procs.hist.get(&pid);
        let cpu_hist: Vec<f32> = hist.map(|h| h.cpu.iter().copied().collect()).unwrap_or_default();
        let mem_hist: Vec<f32> = hist.map(|h| h.mem.iter().copied().collect()).unwrap_or_default();
        let io_hist: Vec<f32> = hist.map(|h| h.io.iter().copied().collect()).unwrap_or_default();
        let io_scale = Graph::new().min_max(64. * 1024.).series(&io_hist, color::disk()).scale(340.);
        let cpu_scale = Graph::new().min_max(10.).series(&cpu_hist, color::cpu()).scale(340.);
        let mem_peak = mem_hist.iter().copied().fold(0.0, f32::max);
        let cores = m.cpu.cores.len().max(1) as f32;

        let sig = |id: &'static str, label: &str, signal: i32, col: Rgba, cx: &mut Context<Self>| {
            button(id, label, col).on_click(cx.listener(move |this, _, _, cx| {
                this.send_signal(pid, signal);
                cx.notify();
            }))
        };
        let renice = |id: &'static str, label: &str, by: i64, cx: &mut Context<Self>| {
            let new = (p.nice + by).min(19);
            button(id, label, color::warn()).on_click(cx.listener(move |this, _, _, cx| {
                this.send_nice(pid, new);
                cx.notify();
            }))
        };

        panel
            .child(div().flex().items_center().child(title("Process", color::text())).child(div().flex_1()).child(
                button("close", "✕", color::muted()).on_click(cx.listener(|this, _, _, cx| {
                    this.procs.selected = None;
                    cx.notify();
                })),
            ))
            .child(big(p.name.clone()).text_lg())
            .child(div().text_xs().font_family(MONO).text_color(color::muted()).line_clamp(4).child(SharedString::from(p.cmd.clone())))
            .child(stat("PID / parent", format!("{pid} / {parent}")))
            .child(stat("Children", children.to_string()))
            .child(stat("User", p.user.clone()))
            .when(!p.unit.is_empty(), |d| d.child(stat("Unit", p.unit.clone())))
            .child(stat("State", format!("{} ({})", p.state, state_name(p.state))))
            .child(stat("Threads / nice", format!("{} / {}", p.threads, p.nice)))
            .child(stat("CPU", format!("{:.1} % ({:.1} % of the whole CPU)", p.cpu, p.cpu / cores)))
            .child(graph_box(Graph::new().max(cpu_scale).series(&cpu_hist, color::cpu()).grid(), 70., Some(format!("{cpu_scale:.0} %"))))
            .child(stat("Memory (peak seen)", format!("{} ({})", bytes(p.mem), bytes(mem_peak as u64))))
            .child(graph_box(Graph::new().min_max(1.).series(&mem_hist, color::mem()).grid(), 70., Some(bytes(mem_peak as u64))))
            .child(stat(
                "Disk read / write",
                format!("{} / {}", crate::model::rate(p.io_read), crate::model::rate(p.io_write)),
            ))
            .when(io_hist.iter().any(|&x| x > 0.0), |d| {
                d.child(graph_box(
                    Graph::new().max(io_scale).series(&io_hist, color::disk()).grid(),
                    50.,
                    Some(crate::model::rate(io_scale)),
                ))
            })
            .when_some(p.vram, |d, v| d.child(stat("VRAM", bytes(v))))
            .child(sub(format!("history since the client saw it: {} samples", cpu_hist.len())))
            .child(div().flex_1())
            .child(title("Signals", color::muted()))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_1()
                    .child(sig("term", "TERM", 15, color::warn(), cx))
                    .child(sig("kill", "KILL", 9, color::bad(), cx))
                    .child(sig("int", "INT", 2, color::text(), cx))
                    .child(sig("hup", "HUP", 1, color::text(), cx))
                    .child(sig("stop", "STOP", 19, color::text(), cx))
                    .child(sig("cont", "CONT", 18, color::text(), cx)),
            )
            .child(title("Priority", color::muted()))
            .child(div().flex().gap_1().child(renice("nice1", "nice +1", 1, cx)).child(renice("nice5", "nice +5", 5, cx)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(pid: u64, ppid: u64, name: &str, cpu: f32) -> Proc {
        Proc {
            pid,
            ppid,
            name: name.into(),
            cmd: String::new(),
            user: "u".into(),
            unit: String::new(),
            state: 'S',
            cpu,
            mem: 0,
            vram: None,
            io_read: 0.0,
            io_write: 0.0,
            threads: 1,
            nice: 0,
        }
    }

    fn model() -> Model {
        Model {
            procs: vec![
                proc(1, 0, "init", 0.0),
                proc(2, 0, "kthreadd", 0.0),
                proc(3, 2, "kworker", 5.0),
                proc(10, 1, "shell", 1.0),
                proc(11, 10, "vim", 9.0),
                proc(12, 10, "make", 3.0),
            ],
            ..Model::default()
        }
    }

    fn names(st: &ProcState) -> Vec<(String, usize)> {
        st.rows.iter().map(|r| (r.proc.name.clone(), r.depth)).collect()
    }

    #[test]
    fn tree_orders_children_by_sort_key() {
        let mut st = ProcState { tree: true, sort: SortKey::Pid, descending: false, ..Default::default() };
        st.rebuild(&model());
        let n = names(&st);
        assert_eq!(n[0], ("init".into(), 0));
        assert_eq!(n[1], ("shell".into(), 1));
        assert_eq!(n[2], ("vim".into(), 2));
        assert_eq!(n[3], ("make".into(), 2));
        st.sort = SortKey::Cpu;
        st.descending = true;
        st.rebuild(&model());
        let n = names(&st);
        let vim = n.iter().position(|x| x.0 == "vim").unwrap();
        let make = n.iter().position(|x| x.0 == "make").unwrap();
        assert!(vim < make, "busier sibling first: {n:?}");
    }

    #[test]
    fn collapse_hides_descendants() {
        let mut st = ProcState { tree: true, ..Default::default() };
        st.collapsed.insert(10);
        st.rebuild(&model());
        let n: Vec<String> = names(&st).into_iter().map(|x| x.0).collect();
        assert!(n.contains(&"shell".to_string()));
        assert!(!n.contains(&"vim".to_string()));
    }

    #[test]
    fn filter_keeps_parent_chain() {
        let mut st = ProcState { tree: true, filter: "vim".into(), ..Default::default() };
        st.rebuild(&model());
        let n: Vec<String> = names(&st).into_iter().map(|x| x.0).collect();
        assert_eq!(n, vec!["init", "shell", "vim"]);
    }

    #[test]
    fn hide_kernel_threads() {
        let mut st = ProcState { hide_kernel: true, ..Default::default() };
        st.rebuild(&model());
        assert!(st.rows.iter().all(|r| r.proc.pid != 2 && r.proc.ppid != 2));
        assert_eq!(st.rows.len(), 4);
    }

    #[test]
    fn history_forgets_exited() {
        let mut st = ProcState::default();
        let mut m = model();
        st.record(&m);
        st.record(&m);
        assert_eq!(st.hist[&11].cpu.len(), 2);
        m.procs.retain(|p| p.pid != 11);
        st.record(&m);
        assert!(!st.hist.contains_key(&11));
    }
}
