//! History page: the per-minute samples hwlensd keeps for days ("history" request,
//! docs/protocol.md). Click a graph to see the busiest processes of that minute.

use std::{
    collections::HashMap,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use gpui::{Context, Div, MouseButton, MouseDownEvent, Rgba, div, prelude::*, px};
use serde_json::{Value, json};

use super::{
    HwlensView,
    widgets::{Graph, MONO, Tracked, card, color, graph_box, legend, stat, sub, title},
};
use crate::model::{bytes, rate};

/// Range buttons: label, seconds.
pub const RANGES: [(&str, i64); 6] = [("1 h", 3600), ("6 h", 6 * 3600), ("24 h", 86400), ("3 d", 3 * 86400), ("7 d", 7 * 86400), ("30 d", 30 * 86400)];
const POINTS: i64 = 720;
/// How often an open page asks for fresh data (samples arrive once a minute).
pub const REFRESH_SECS: u64 = 30;

pub struct HistState {
    pub range: i64,
    pub data: Option<HistData>,
    pub error: Option<String>,
    /// The picked minute and its sample (`{"t","v","top"}`).
    pub selected: Option<i64>,
    pub sample: Option<Value>,
    pub requested: Option<Instant>,
}

impl Default for HistState {
    fn default() -> Self {
        HistState { range: RANGES[1].1, data: None, error: None, selected: None, sample: None, requested: None }
    }
}

/// A range answer laid out on a regular time grid: `series[k][i]` is the value at `from + i * step`,
/// None where no sample exists (the daemon was not running).
pub struct HistData {
    pub from: i64,
    pub step: i64,
    pub len: usize,
    pub days: i64,
    pub oldest: Option<i64>,
    pub series: HashMap<String, Vec<Option<f32>>>,
}

impl HistData {
    pub fn parse(v: &Value) -> Result<HistData, String> {
        if v["ok"] != true {
            return Err(v["error"].as_str().unwrap_or("history request failed").to_owned());
        }
        let from = v["from"].as_i64().ok_or("no from")?;
        let to = v["to"].as_i64().ok_or("no to")?;
        let step = v["step"].as_i64().filter(|s| *s > 0).ok_or("no step")?;
        let len = ((to - from) / step + 1).max(2) as usize;
        let times: Vec<i64> = v["t"].as_array().map(|a| a.iter().filter_map(Value::as_i64).collect()).unwrap_or_default();
        let mut series = HashMap::new();
        if let Some(obj) = v["series"].as_object() {
            for (k, vals) in obj {
                let mut grid = vec![None; len];
                for (t, val) in times.iter().zip(vals.as_array().into_iter().flatten()) {
                    let i = (t - from) / step;
                    if (0..len as i64).contains(&i) {
                        grid[i as usize] = val.as_f64().map(|x| x as f32);
                    }
                }
                series.insert(k.clone(), grid);
            }
        }
        Ok(HistData { from, step, len, days: v["days"].as_i64().unwrap_or(0), oldest: v["oldest"].as_i64(), series })
    }

    pub fn span(&self) -> i64 {
        (self.len as i64 - 1) * self.step
    }

    pub fn has(&self, key: &str) -> bool {
        self.series.get(key).is_some_and(|v| v.iter().any(Option::is_some))
    }

    /// Values for drawing: gaps become zero.
    pub fn values(&self, key: &str) -> Vec<f32> {
        self.series.get(key).map(|v| v.iter().map(|x| x.unwrap_or(0.0)).collect()).unwrap_or_default()
    }

    /// Average and peak over the samples that exist.
    pub fn stats(&self, key: &str) -> (f32, f32) {
        let present: Vec<f32> = self.series.get(key).into_iter().flatten().flatten().copied().collect();
        if present.is_empty() {
            return (0.0, 0.0);
        }
        (present.iter().sum::<f32>() / present.len() as f32, present.iter().copied().fold(0.0, f32::max))
    }

    /// The grid time for a horizontal position (0..=1).
    pub fn time_at(&self, fraction: f32) -> i64 {
        let i = (fraction.clamp(0.0, 1.0) * (self.len - 1) as f32).round() as i64;
        self.from + i * self.step
    }

    pub fn fraction_of(&self, t: i64) -> Option<f32> {
        let f = (t - self.from) as f32 / self.span().max(1) as f32;
        (0.0..=1.0).contains(&f).then_some(f)
    }
}

pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Local wall clock time of a unix time: "15:42", or "11.11 15:42" with the date.
pub fn local_time(t: i64, with_date: bool) -> String {
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let tt = t as libc::time_t;
    // SAFETY: both pointers are valid for the call; localtime_r is the reentrant variant.
    if unsafe { libc::localtime_r(&tt, &mut tm) }.is_null() {
        return t.to_string();
    }
    if with_date {
        format!("{:02}.{:02} {:02}:{:02}", tm.tm_mday, tm.tm_mon + 1, tm.tm_hour, tm.tm_min)
    } else {
        format!("{:02}:{:02}", tm.tm_hour, tm.tm_min)
    }
}

/// One graph card: title, the series it shows and how to format their values.
struct Chart {
    title: &'static str,
    accent: Rgba,
    lines: Vec<(&'static str, &'static str, Rgba)>, // key, label, color
    max: Option<f32>,
    min_max: f32,
    fmt: fn(f32) -> String,
}

fn pct(v: f32) -> String {
    format!("{v:.1} %")
}
fn celsius(v: f32) -> String {
    format!("{v:.0} °C")
}
fn watts(v: f32) -> String {
    format!("{v:.0} W")
}
fn plain(v: f32) -> String {
    format!("{v:.2}")
}

fn charts() -> Vec<Chart> {
    vec![
        Chart { title: "CPU", accent: color::cpu(), lines: vec![("cpu", "avg", color::cpu()), ("cpu_max", "peak", color::series(1)), ("iowait", "iowait", color::warn())], max: Some(100.), min_max: 1., fmt: pct },
        Chart { title: "Memory", accent: color::mem(), lines: vec![("mem", "used", color::mem()), ("swap", "swap", color::up())], max: Some(100.), min_max: 1., fmt: pct },
        Chart { title: "Network", accent: color::down(), lines: vec![("net_rx", "down", color::down()), ("net_tx", "up", color::up())], max: None, min_max: 1024., fmt: rate },
        Chart { title: "Disk I/O", accent: color::disk(), lines: vec![("disk_read", "read", color::disk()), ("disk_write", "write", color::up())], max: None, min_max: 1024., fmt: rate },
        Chart { title: "GPU", accent: color::gpu(), lines: vec![("gpu", "load", color::gpu()), ("vram", "vram", color::series(1))], max: Some(100.), min_max: 1., fmt: pct },
        Chart { title: "Temperature", accent: color::temp(), lines: vec![("cpu_temp", "cpu", color::temp()), ("gpu_temp", "gpu", color::gpu())], max: None, min_max: 50., fmt: celsius },
        Chart { title: "Power", accent: color::warn(), lines: vec![("cpu_watts", "cpu", color::warn()), ("gpu_watts", "gpu", color::gpu())], max: None, min_max: 10., fmt: watts },
        Chart { title: "Pressure (some)", accent: color::bad(), lines: vec![("psi_cpu", "cpu", color::cpu()), ("psi_mem", "mem", color::mem()), ("psi_io", "io", color::disk())], max: None, min_max: 1., fmt: pct },
        Chart { title: "Load average (1 min)", accent: color::muted(), lines: vec![("load1", "load", color::muted())], max: None, min_max: 1., fmt: plain },
    ]
}

impl HwlensView {
    /// Asks for the selected range; the answer arrives as Event::History.
    pub(super) fn history_request(&mut self) {
        let to = now();
        let body = json!({"from": to - self.history.range, "to": to, "points": POINTS});
        self.history.requested = Some(Instant::now());
        if let Err(e) = self.requests.history(body) {
            self.history.error = Some(format!("{e:#}"));
        }
    }

    pub(super) fn history_reply(&mut self, v: Value) {
        if v.get("sample").is_some() {
            self.history.sample = v.get("sample").filter(|s| s.is_object()).cloned();
            return;
        }
        match HistData::parse(&v) {
            Ok(d) => {
                self.history.data = Some(d);
                self.history.error = None;
            }
            Err(e) => self.history.error = Some(e),
        }
    }

    fn history_range(&mut self, range: i64, cx: &mut Context<Self>) {
        self.history.range = range;
        self.history_request();
        cx.notify();
    }

    fn history_pick(&mut self, fraction: f32, cx: &mut Context<Self>) {
        let Some(d) = &self.history.data else { return };
        let t = d.time_at(fraction);
        self.history.selected = Some(t);
        self.history.sample = None;
        if let Err(e) = self.requests.history(json!({"at": t})) {
            self.history.error = Some(format!("{e:#}"));
        }
        cx.notify();
    }

    pub(super) fn history_page(&mut self, cx: &mut Context<Self>) -> Div {
        let h = &self.history;
        let buttons = RANGES.iter().map(|&(label, secs)| {
            let active = h.range == secs;
            div()
                .id(("range", secs as usize))
                .px_2()
                .py(px(2.))
                .rounded_md()
                .cursor_pointer()
                .text_xs()
                .when(active, |d| d.bg(color::selected()).text_color(color::text()))
                .when(!active, |d| d.text_color(color::muted()).hover(|s| s.bg(color::hover())))
                .on_click(cx.listener(move |this, _, _, cx| this.history_range(secs, cx)))
                .child(label)
        });
        let info = match (&h.data, &h.error) {
            (_, Some(e)) => div().text_xs().text_color(color::bad()).child(e.clone()),
            (Some(d), None) => div().text_xs().text_color(color::muted()).child(format!(
                "one point per {} · kept {} days{}",
                if d.step >= 3600 { format!("{} h", d.step / 3600) } else { format!("{} min", d.step / 60) },
                d.days,
                d.oldest.map(|o| format!(" · since {}", local_time(o, true))).unwrap_or_default()
            )),
            (None, None) => div().text_xs().text_color(color::muted()).child("loading…"),
        };
        let header = card()
            .flex_row()
            .items_center()
            .gap_2()
            .child(title("History", color::text()))
            .child(div().flex().gap_1().children(buttons))
            .child(div().flex_1())
            .child(info);

        let Some(d) = &h.data else {
            return div().flex().flex_col().gap_3().child(header);
        };
        let with_date = h.range > 86400;
        let marker = h.selected.and_then(|t| d.fraction_of(t));

        let mut cards = Vec::new();
        for (i, c) in charts().into_iter().enumerate() {
            let lines: Vec<_> = c.lines.iter().filter(|(k, _, _)| d.has(k)).collect();
            if lines.is_empty() {
                continue;
            }
            let mut g = Graph::new().min_max(c.min_max).fit().grid().marker(marker);
            if let Some(m) = c.max {
                g = g.max(m);
            }
            let mut legends = div().flex().flex_wrap().gap_3();
            for (key, label, col) in &lines {
                g = g.series(&d.values(key), *col);
                let (a, p) = d.stats(key);
                legends = legends.child(legend(*col, *label, format!("avg {} · max {}", (c.fmt)(a), (c.fmt)(p))));
            }
            let track = Tracked::default();
            g = g.track(track.clone());
            let label = c.max.map(|m| (c.fmt)(m)).or_else(|| Some((c.fmt)(g.scale(d.len as f32 * g.step))));
            let plot = div()
                .id(("hist", i))
                .cursor_crosshair()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                        if let Some(b) = track.get() {
                            let f = f32::from(ev.position.x - b.left()) / f32::from(b.size.width).max(1.0);
                            this.history_pick(f, cx);
                        }
                    }),
                )
                .child(graph_box(g, 120., label));
            let axis = div()
                .flex()
                .text_xs()
                .text_color(color::faint())
                .child(local_time(d.from, with_date))
                .child(div().flex_1())
                .child(local_time(d.from + d.span() / 2, with_date))
                .child(div().flex_1())
                .child(local_time(d.from + d.span(), with_date));
            cards.push(card().gap_2().child(title(c.title, c.accent)).child(plot).child(axis).child(legends));
        }
        let (left, right): (Vec<_>, Vec<_>) = cards.into_iter().enumerate().partition(|(i, _)| i % 2 == 0);
        let column = |v: Vec<(usize, Div)>| div().flex_1().min_w(px(420.)).flex().flex_col().gap_3().children(v.into_iter().map(|(_, c)| c));

        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(header)
            .child(self.history_details())
            .child(div().flex().flex_wrap().gap_3().child(column(left)).child(column(right)))
    }

    fn history_details(&self) -> Div {
        let h = &self.history;
        let Some(t) = h.selected else {
            return card().child(sub("Click a graph to see that minute: averages and the busiest processes."));
        };
        let Some(s) = &h.sample else {
            return card().child(sub(format!("{} · loading…", local_time(t, true))));
        };
        let st = s["t"].as_i64().unwrap_or(t);
        let v = &s["v"];
        let num = |k: &str| v[k].as_f64().map(|x| x as f32);
        let mut values = div().flex().flex_col().gap_1().min_w(px(220.));
        let rows: [(&str, &str, fn(f32) -> String); 8] = [
            ("CPU avg / peak", "cpu", pct),
            ("Memory", "mem", pct),
            ("Swap", "swap", pct),
            ("CPU temperature", "cpu_temp", celsius),
            ("GPU", "gpu", pct),
            ("Network down", "net_rx", rate),
            ("Network up", "net_tx", rate),
            ("Load average", "load1", plain),
        ];
        for (label, key, f) in rows {
            if let Some(x) = num(key) {
                let text = if key == "cpu" { format!("{} / {}", f(x), num("cpu_max").map(pct).unwrap_or_default()) } else { f(x) };
                values = values.child(stat(label, text));
            }
        }
        let top = |name: &'static str, key: &str, col: Rgba, f: fn(f32) -> String| {
            let rows = s["top"][key].as_array().cloned().unwrap_or_default();
            let mut c = div().flex_1().min_w(px(200.)).flex().flex_col().gap_1().child(title(name, col));
            if rows.is_empty() {
                c = c.child(sub("—"));
            }
            for r in rows {
                let pid = r[0].as_u64().unwrap_or(0);
                let pname = r[1].as_str().unwrap_or("?").to_owned();
                let val = r[2].as_f64().unwrap_or(0.0) as f32;
                c = c.child(
                    div()
                        .flex()
                        .gap_2()
                        .text_xs()
                        .font_family(MONO)
                        .child(div().w(px(64.)).text_right().text_color(color::faint()).child(pid.to_string()))
                        .child(div().flex_1().truncate().child(pname))
                        .child(div().child(f(val))),
                );
            }
            c
        };
        let mem_bytes = |v: f32| bytes(v as u64);
        card()
            .gap_2()
            .child(title(format!("{} · minute averages", local_time(st, true)), color::text()))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_4()
                    .child(values)
                    .child(top("Top CPU (% of a core)", "cpu", color::cpu(), pct))
                    .child(top("Top memory (peak)", "mem", color::mem(), mem_bytes))
                    .child(top("Top disk I/O", "io", color::disk(), rate)),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_places_samples_and_keeps_gaps() {
        let v = json!({"reply":"history","ok":true,"days":7,"oldest":0,"from":1000,"to":1300,"step":60,
            "t":[1000,1120,1300],"series":{"cpu":[10.0,30.0,50.0],"mem":[1.0,null,2.0]}});
        let d = HistData::parse(&v).unwrap();
        assert_eq!(d.len, 6);
        assert_eq!(d.series["cpu"], vec![Some(10.0), None, Some(30.0), None, None, Some(50.0)]);
        assert_eq!(d.series["mem"][2], None);
        assert_eq!(d.stats("cpu"), (30.0, 50.0));
        assert_eq!(d.values("cpu")[1], 0.0);
        assert_eq!(d.time_at(0.0), 1000);
        assert_eq!(d.time_at(1.0), 1300);
        assert_eq!(d.time_at(0.41), 1120);
        assert_eq!(d.fraction_of(1150), Some(0.5));
        assert_eq!(d.fraction_of(5000), None);
    }

    #[test]
    fn errors_are_reported() {
        let v = json!({"reply":"history","ok":false,"error":"history is disabled"});
        assert_eq!(HistData::parse(&v).err().as_deref(), Some("history is disabled"));
    }
}
