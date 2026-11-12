//! Debug page: what this hwlens-gui process holds. Memory by mapping (/proc/self/smaps),
//! a timeline sampled every 10 s since start, event counters that may explain growth
//! (frames, resizes, page switches, reconnects) and the size of the client-side data.
//! "Snapshot" writes all of it to ~/.cache/hwlens/ for later comparison.

use std::{collections::VecDeque, fmt::Write as _, path::PathBuf};

use gpui::{Context, Div, div, prelude::*, px};

use super::{
    HwlensView,
    history::{local_time, now},
    procs::button,
    widgets::{Graph, MONO, card, chip, color, graph_card, overlay, overlay_row, legend, stat, sub, title},
};

/// Timeline points kept: one per SAMPLE_TICKS ticks, a day at 1 s ticks.
const TIMELINE: usize = 8640;
pub const SAMPLE_TICKS: u64 = 10;

#[derive(Clone, Default)]
pub struct MemGroup {
    pub name: String,
    pub rss_kb: u64,
    pub pss_kb: u64,
    pub anon_kb: u64,
    pub maps: usize,
}

#[derive(Clone, Default)]
pub struct MemReport {
    pub groups: Vec<MemGroup>,
    pub rss_kb: u64,
    pub pss_kb: u64,
    pub anon_kb: u64,
    pub file_kb: u64,
    pub gpu_kb: u64,
    pub hwm_kb: u64,
    pub threads: u64,
    pub fds: usize,
}

/// The group a mapping counts under.
fn group_of(path: &str) -> String {
    match path {
        "" => "anonymous (malloc arenas, thread stacks)".into(),
        "[heap]" => "[heap]".into(),
        p if p.starts_with("/dev/nvidia") => format!("GPU driver ({p})"),
        p if p.starts_with("/dev/dri/") => format!("GPU DRM ({p})"),
        p if p.starts_with("[stack") => "[stack]".into(),
        p if p.starts_with("memfd:") => "memfd (shared buffers)".into(),
        p if p.starts_with('[') => p.into(),
        p if p.contains(".so") => format!("lib {}", p.rsplit('/').next().unwrap_or(p)),
        p => p.into(),
    }
}

/// Parses smaps text into groups, largest RSS first.
pub fn parse_smaps(text: &str) -> MemReport {
    let mut r = MemReport::default();
    let mut groups: std::collections::HashMap<String, MemGroup> = Default::default();
    let mut current = String::new();
    for line in text.lines() {
        let mut it = line.split_whitespace();
        let Some(first) = it.next() else { continue };
        if first.contains('-') && first.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
            // "start-end perms offset dev inode [path]"
            let path = line.split_whitespace().skip(5).collect::<Vec<_>>().join(" ");
            current = group_of(&path);
            groups.entry(current.clone()).or_insert_with(|| MemGroup { name: current.clone(), ..Default::default() }).maps += 1;
            continue;
        }
        let kb = || it.clone().next().and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
        let g = groups.get_mut(&current);
        match first {
            "Rss:" => {
                let v = kb();
                r.rss_kb += v;
                if current.starts_with("GPU") {
                    r.gpu_kb += v;
                }
                if let Some(g) = g {
                    g.rss_kb += v;
                }
            }
            "Pss:" => {
                let v = kb();
                r.pss_kb += v;
                if let Some(g) = g {
                    g.pss_kb += v;
                }
            }
            "Anonymous:" => {
                let v = kb();
                r.anon_kb += v;
                if let Some(g) = g {
                    g.anon_kb += v;
                }
            }
            _ => {}
        }
    }
    r.file_kb = r.rss_kb.saturating_sub(r.anon_kb);
    r.groups = groups.into_values().filter(|g| g.rss_kb > 0).collect();
    r.groups.sort_by(|a, b| b.rss_kb.cmp(&a.rss_kb).then(a.name.cmp(&b.name)));
    r
}

pub fn read_self() -> MemReport {
    let mut r = parse_smaps(&std::fs::read_to_string("/proc/self/smaps").unwrap_or_default());
    for line in std::fs::read_to_string("/proc/self/status").unwrap_or_default().lines() {
        let num = || line.split_whitespace().nth(1).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
        if line.starts_with("VmHWM:") {
            r.hwm_kb = num();
        } else if line.starts_with("Threads:") {
            r.threads = num();
        }
    }
    r.fds = std::fs::read_dir("/proc/self/fd").map(|d| d.count()).unwrap_or(0);
    r
}

/// Time, RSS, GPU-mapped and anonymous memory (kB).
#[derive(Clone, Copy)]
pub struct Point {
    pub t: i64,
    pub rss: u64,
    pub gpu: u64,
    pub anon: u64,
}

pub struct DebugState {
    pub started: i64,
    pub timeline: VecDeque<Point>,
    pub last: Option<MemReport>,
    pub frames: u64,
    pub resizes: u64,
    pub last_size: Option<(i32, i32)>,
    pub page_switches: u64,
    pub reconnects: u64,
    pub models: u64,
    pub last_snapshot: Option<PathBuf>,
}

impl Default for DebugState {
    fn default() -> Self {
        DebugState {
            started: now(),
            timeline: VecDeque::new(),
            last: None,
            frames: 0,
            resizes: 0,
            last_size: None,
            page_switches: 0,
            reconnects: 0,
            models: 0,
            last_snapshot: None,
        }
    }
}

impl DebugState {
    pub fn sample(&mut self) {
        let r = read_self();
        self.timeline.push_back(Point { t: now(), rss: r.rss_kb, gpu: r.gpu_kb, anon: r.anon_kb });
        if self.timeline.len() > TIMELINE {
            self.timeline.pop_front();
        }
        self.last = Some(r);
    }

    /// Counts a window size change (called from render).
    pub fn saw_size(&mut self, w: f32, h: f32) {
        let s = (w.round() as i32, h.round() as i32);
        if self.last_size.is_some_and(|l| l != s) {
            self.resizes += 1;
        }
        self.last_size = Some(s);
    }
}

fn mb(kb: u64) -> String {
    if kb >= 1024 * 1024 { format!("{:.2} GiB", kb as f64 / 1048576.) } else { format!("{:.1} MiB", kb as f64 / 1024.) }
}

impl HwlensView {
    /// Sizes of what the client keeps between ticks: (name, entries, approximate bytes).
    fn client_data(&self) -> Vec<(&'static str, usize, usize)> {
        let hist_samples: usize = self.procs.hist.values().map(|h| h.cpu.len() + h.mem.len() + h.io.len() + 2 * h.rw.len()).sum();
        let sensor_samples: usize = self.sensor_hist.values().map(|h| h.len()).sum();
        let hist_points: usize = self.history.data.as_ref().map(|d| d.series.len() * d.len).unwrap_or(0);
        let journal = self.svc.journal.as_ref().map(|j| j.2.len()).unwrap_or(0);
        vec![
            ("Process history (per pid)", self.procs.hist.len(), hist_samples * 4 + self.procs.hist.len() * 128),
            ("Sensor history", self.sensor_hist.len(), sensor_samples * 4),
            ("Process rows", self.procs.rows.len(), self.procs.rows.len() * 256),
            ("Socket rows", self.conn_rows.len(), self.conn_rows.len() * 160),
            ("systemd units", self.svc.units.len(), self.svc.units.len() * 256),
            ("Service journal text", usize::from(journal > 0), journal),
            ("History page points", hist_points, hist_points * 8),
            ("Debug timeline", self.debug.timeline.len(), self.debug.timeline.len() * std::mem::size_of::<Point>()),
        ]
    }

    pub(super) fn debug_snapshot(&mut self) {
        self.debug.sample();
        let r = self.debug.last.clone().unwrap_or_default();
        let d = &self.debug;
        let mut s = String::new();
        let _ = writeln!(s, "hwlens-gui self snapshot {} (pid {})", local_time(now(), true), std::process::id());
        let _ = writeln!(s, "running since {} ({} s)", local_time(d.started, true), now() - d.started);
        let _ = writeln!(
            s,
            "RSS {} · PSS {} · anonymous {} · file/driver {} · GPU-mapped {} · peak RSS {} · threads {} · fds {}",
            mb(r.rss_kb),
            mb(r.pss_kb),
            mb(r.anon_kb),
            mb(r.file_kb),
            mb(r.gpu_kb),
            mb(r.hwm_kb),
            r.threads,
            r.fds
        );
        let _ = writeln!(
            s,
            "frames {} · window resizes {} · page switches {} · reconnects {} · models {}",
            d.frames, d.resizes, d.page_switches, d.reconnects, d.models
        );
        let _ = writeln!(s, "\n== memory by mapping (RSS, PSS, anonymous kB, mappings) ==");
        for g in &r.groups {
            let _ = writeln!(s, "{:>10} {:>10} {:>10} {:>5}  {}", g.rss_kb, g.pss_kb, g.anon_kb, g.maps, g.name);
        }
        let _ = writeln!(s, "\n== client data (entries, approx bytes) ==");
        for (name, n, b) in self.client_data() {
            let _ = writeln!(s, "{n:>10} {b:>12}  {name}");
        }
        let _ = writeln!(s, "\n== timeline (unix time, RSS kB, GPU-mapped kB, anonymous kB) ==");
        for p in &self.debug.timeline {
            let _ = writeln!(s, "{},{},{},{}", p.t, p.rss, p.gpu, p.anon);
        }

        let dir = std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
            .unwrap_or_else(std::env::temp_dir)
            .join("hwlens");
        let file = dir.join(format!("gui-snapshot-{}.txt", local_time(now(), true).replace([' ', ':', '.'], "-")));
        self.message = Some(match std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&file, s)) {
            Ok(()) => {
                self.debug.last_snapshot = Some(file.clone());
                (true, format!("snapshot saved: {}", file.display()))
            }
            Err(e) => (false, format!("snapshot: {}: {e}", file.display())),
        });
    }

    pub(super) fn debug_page(&mut self, cx: &mut Context<Self>) -> Div {
        if self.debug.last.is_none() {
            self.debug.sample();
        }
        let r = self.debug.last.clone().unwrap_or_default();
        let d = &self.debug;

        let actions = div()
            .flex()
            .gap_2()
            .child(button("dbg-snap", "Snapshot to file", color::text()).on_click(cx.listener(|this, _, _, cx| {
                this.debug_snapshot();
                cx.notify();
            })))
            .child(button("dbg-refresh", "Refresh now", color::muted()).on_click(cx.listener(|this, _, _, cx| {
                this.debug.sample();
                cx.notify();
            })));

        let summary = card()
            .gap_2()
            .child(div().flex().items_center().child(title("This process", color::text())).child(div().flex_1()).child(actions))
            .child(stat("Resident (RSS) / proportional (PSS)", format!("{} / {}", mb(r.rss_kb), mb(r.pss_kb))))
            .child(stat("GPU driver mappings", mb(r.gpu_kb)))
            .child(stat("Anonymous (heap, arenas, stacks)", mb(r.anon_kb)))
            .child(stat("Peak RSS", mb(r.hwm_kb)))
            .child(stat("Threads / open files", format!("{} / {}", r.threads, r.fds)))
            .child(stat("Running since", format!("{} ({})", local_time(d.started, true), crate::model::duration(now() - d.started))))
            .children(d.last_snapshot.as_ref().map(|p| sub(format!("last snapshot: {}", p.display()))));

        let counters = card()
            .gap_2()
            .child(title("Events since start", color::text()))
            .child(stat("Frames rendered", d.frames.to_string()))
            .child(stat("Window size changes", d.resizes.to_string()))
            .child(stat("Page switches", d.page_switches.to_string()))
            .child(stat("Reconnects to hwlensd", d.reconnects.to_string()))
            .child(stat("Snapshots from hwlensd", d.models.to_string()))
            .child(sub("Memory that grows with one of these counters points at its cause."));

        let rss: Vec<f32> = d.timeline.iter().map(|p| p.rss as f32 * 1024.).collect();
        let gpu: Vec<f32> = d.timeline.iter().map(|p| p.gpu as f32 * 1024.).collect();
        let anon: Vec<f32> = d.timeline.iter().map(|p| p.anon as f32 * 1024.).collect();
        let mut g = Graph::new()
            .min_max(64. * 1048576.)
            .grid()
            .fit()
            .fmt(super::widgets::fmt_bytes)
            .series(&rss, color::text())
            .name("RSS")
            .series(&gpu, color::gpu())
            .name("GPU driver")
            .series(&anon, color::mem())
            .name("anonymous");
        if let Some(p) = d.timeline.back() {
            g = g.time(p.t as f64, SAMPLE_TICKS as f64 * self.model.as_ref().map(|m| m.update_ms as f64 / 1000.).unwrap_or(1.0));
        }
        let scale = crate::model::bytes(g.scale(rss.len() as f32 * g.step) as u64);
        let timeline = graph_card(
            g,
            200.,
            overlay()
                .child(
                    overlay_row()
                        .child(chip(title(format!("Memory over time · every {SAMPLE_TICKS} s · {} points", d.timeline.len()), color::text())))
                        .child(div().flex_1())
                        .child(chip(sub(scale))),
                )
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(chip(legend(color::text(), "RSS", mb(r.rss_kb))))
                        .child(chip(legend(color::gpu(), "GPU driver", mb(r.gpu_kb))))
                        .child(chip(legend(color::mem(), "anonymous", mb(r.anon_kb)))),
                )
                .children(d.timeline.front().map(|p| chip(sub(format!("first point {}: RSS {} · GPU {}", local_time(p.t, true), mb(p.rss), mb(p.gpu)))))),
        );

        let mut maps = card().gap_1().child(
            div()
                .flex()
                .text_xs()
                .text_color(color::muted())
                .child(title("Memory by mapping", color::text()).flex_1())
                .child(div().w(px(90.)).text_right().child("RSS"))
                .child(div().w(px(90.)).text_right().child("anonymous"))
                .child(div().w(px(50.)).text_right().child("maps")),
        );
        for g in r.groups.iter().take(18) {
            maps = maps.child(
                div()
                    .flex()
                    .text_xs()
                    .font_family(MONO)
                    .child(div().flex_1().min_w_0().truncate().child(g.name.clone()))
                    .child(div().w(px(90.)).text_right().child(mb(g.rss_kb)))
                    .child(div().w(px(90.)).text_right().text_color(color::muted()).child(mb(g.anon_kb)))
                    .child(div().w(px(50.)).text_right().text_color(color::faint()).child(g.maps.to_string())),
            );
        }

        let mut data = card().gap_1().child(title("Client data", color::text()));
        for (name, n, b) in self.client_data() {
            data = data.child(stat(name, format!("{n} · ~{}", crate::model::bytes(b as u64))));
        }

        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(div().flex().flex_wrap().gap_3().child(div().flex_1().min_w(px(420.)).child(summary)).child(div().flex_1().min_w(px(420.)).child(counters)))
            .child(timeline)
            .child(div().flex().flex_wrap().gap_3().child(div().flex_1().min_w(px(420.)).child(maps)).child(div().flex_1().min_w(px(420.)).child(data)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smaps_groups() {
        let text = "\
55d0-55e0 rw-p 00000000 00:00 0                          [heap]
Rss:                 100 kB
Pss:                 100 kB
Anonymous:           100 kB
7f00-7f10 rw-s 00000000 00:05 12                         /dev/nvidiactl
Rss:                1000 kB
Pss:                 900 kB
Anonymous:             0 kB
7f10-7f20 rw-s 00000000 00:05 12                         /dev/nvidiactl
Rss:                 500 kB
Pss:                 500 kB
Anonymous:             0 kB
7f20-7f30 r-xp 00000000 08:01 99                         /usr/lib/libc.so.6
Rss:                  40 kB
Pss:                  10 kB
Anonymous:             0 kB
7f30-7f40 rw-p 00000000 00:00 0
Rss:                   8 kB
Pss:                   8 kB
Anonymous:             8 kB
";
        let r = parse_smaps(text);
        assert_eq!(r.rss_kb, 1648);
        assert_eq!(r.gpu_kb, 1500);
        assert_eq!(r.anon_kb, 108);
        assert_eq!(r.groups[0].name, "GPU driver (/dev/nvidiactl)");
        assert_eq!(r.groups[0].maps, 2);
        assert_eq!(r.groups[0].rss_kb, 1500);
        assert!(r.groups.iter().any(|g| g.name == "lib libc.so.6" && g.pss_kb == 10));
        assert!(r.groups.iter().any(|g| g.name.starts_with("anonymous") && g.rss_kb == 8));
    }
}
