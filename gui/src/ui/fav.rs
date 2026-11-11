//! Favorites: values pinned from any page (☆ next to a row) and shown live on the overview.
//! A favorite is a key such as `cpu.temp` or `hwmon.nct6798/fan/fan2`; `resolve` turns a key
//! into its label and current value, and the pages build their pinnable rows from it too, so
//! a pinned value always reads like the row it came from.

use std::{collections::HashSet, path::PathBuf, rc::Rc};

use gpui::{App, Context, Div, SharedString, Stateful, WeakEntity, div, prelude::*, px};
use serde_json::{Value, json};

use super::{
    HwlensView,
    widgets::{MONO, card, color, stat, sub, title},
};
use crate::{
    model::{Model, bytes, duration, rate},
    services::{Scope, Unit},
};

/// The favorites file: `$XDG_CONFIG_HOME/hwlens/gui.json` (or `~/.config/...`).
fn config_file() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("hwlens").join("gui.json"))
}

pub fn load() -> Vec<String> {
    let Some(text) = config_file().and_then(|f| std::fs::read_to_string(f).ok()) else { return Vec::new() };
    let v: Value = serde_json::from_str(&text).unwrap_or_default();
    v["favorites"].as_array().into_iter().flatten().filter_map(|k| k.as_str().map(str::to_owned)).collect()
}

pub fn save(keys: &[String]) -> Result<(), String> {
    let file = config_file().ok_or("no home directory")?;
    // Keep whatever else the file holds.
    let mut v: Value = std::fs::read_to_string(&file).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_else(|| json!({}));
    if !v.is_object() {
        v = json!({});
    }
    v["favorites"] = json!(keys);
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let tmp = file.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(&v).unwrap_or_default()).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &file).map_err(|e| format!("{}: {e}", file.display()))
}

pub struct Fav {
    pub label: String,
    pub value: String,
}

fn fav(label: impl Into<String>, value: impl Into<String>) -> Option<Fav> {
    Some(Fav { label: label.into(), value: value.into() })
}

fn last(v: &[f32]) -> f32 {
    v.last().copied().unwrap_or(0.0)
}

fn or_dash(v: Option<String>) -> String {
    v.unwrap_or_else(|| "—".into())
}

/// Label and current value of a favorite key; None when the key names nothing known
/// (an unknown key, or a disk or interface that is gone).
pub fn resolve(m: &Model, units: &[Unit], key: &str) -> Option<Fav> {
    let (group, rest) = key.split_once('.')?;
    let c = &m.cpu;
    match group {
        "cpu" => match rest {
            "usage" => fav("CPU usage", format!("{:.0} %", last(&c.total))),
            "freq" => fav("CPU frequency", c.hz.clone()),
            "load" => fav("Load average", format!("{:.2}  {:.2}  {:.2}", c.load_avg[0], c.load_avg[1], c.load_avg[2])),
            "temp" => fav("CPU package temperature", or_dash(c.temp.map(|t| format!("{t:.0} °C")))),
            "power" => fav("CPU package power", or_dash(c.watts.map(|w| format!("{w:.1} W")))),
            "psi" => fav("CPU pressure (some avg10)", or_dash(c.psi.map(|p| format!("{:.2} %", p[0])))),
            _ => None,
        },
        "mem" => {
            let x = &m.mem;
            let t = x.total.max(1) as f32;
            match rest {
                "used" => fav("Memory used", format!("{} of {} ({:.0} %)", bytes(x.used), bytes(x.total), x.used as f32 * 100. / t)),
                "available" => fav("Memory available", format!("{} ({:.0} %)", bytes(x.available), x.available as f32 * 100. / t)),
                "cached" => fav("Memory cached", bytes(x.cached)),
                "swap" => fav(
                    "Swap used",
                    if x.swap_total > 0 {
                        format!("{} of {} ({:.0} %)", bytes(x.swap_used), bytes(x.swap_total), x.swap_used as f32 * 100. / x.swap_total as f32)
                    } else {
                        "no swap".into()
                    },
                ),
                "psi" => fav("Memory pressure (some avg10)", or_dash(c.psi.map(|p| format!("{:.2} %", p[1])))),
                "io_psi" => fav("I/O pressure (some avg10)", or_dash(c.psi.map(|p| format!("{:.2} %", p[2])))),
                _ => None,
            }
        }
        "sys" => {
            let s = m.sys.as_ref()?;
            let per_s = |v: Option<f64>| or_dash(v.map(|x| format!("{x:.0}/s")));
            match rest {
                "uptime" => fav("Uptime", duration(s.uptime as i64)),
                "procs" => fav("Processes", m.procs.len().to_string()),
                "threads" => {
                    let t = s.threads?;
                    let sleeping = t.saturating_sub(s.procs_running + s.procs_blocked);
                    fav("Threads total / running / sleeping", format!("{t} / {} / {sleeping}", s.procs_running))
                }
                "blocked" => fav("Blocked on I/O (D state)", s.procs_blocked.to_string()),
                "ctxt" => fav("Context switches", per_s(s.ctxt_per_s)),
                "intr" => fav("Interrupts", per_s(s.intr_per_s)),
                "forks" => fav("New processes", or_dash(s.forks_per_s.map(|x| format!("{x:.1}/s")))),
                "files" => fav(
                    "Open files",
                    if s.max_files > 0 && s.max_files < 1 << 40 { format!("{} of {}", s.open_files, s.max_files) } else { s.open_files.to_string() },
                ),
                _ => None,
            }
        }
        "gpu" => {
            let (i, what) = rest.split_once('.')?;
            let g = m.gpus.get(i.parse::<usize>().ok()?)?;
            let name = |l: &str| format!("GPU {} · {l}", g.name);
            match what {
                "load" => fav(name("load"), format!("{:.0} %", last(&g.util))),
                "temp" => fav(name("temperature"), or_dash(g.temp.map(|t| format!("{t:.0} °C")))),
                "power" => fav(
                    name("power"),
                    match (g.power, g.power_max) {
                        (Some(p), Some(mx)) => format!("{p:.1} W of {mx:.0} W"),
                        (Some(p), None) => format!("{p:.1} W"),
                        _ => "—".into(),
                    },
                ),
                "vram" => {
                    let p = if g.mem_total > 0 { g.mem_used as f32 * 100. / g.mem_total as f32 } else { 0. };
                    fav(name("VRAM"), format!("{} of {} ({p:.0} %)", bytes(g.mem_used), bytes(g.mem_total)))
                }
                "clock" => fav(
                    name("core / memory clock"),
                    format!("{} / {}", or_dash(g.clock_mhz.map(|c| format!("{c} MHz"))), or_dash(g.mem_clock_mhz.map(|c| format!("{c} MHz")))),
                ),
                "fan" => fav(name("fan"), or_dash(g.fan.map(|f| format!("{f} %")))),
                _ => None,
            }
        }
        "net" => {
            // The interface name may contain dots; the field is after the last one.
            let (iface, what) = rest.rsplit_once('.')?;
            let n = m.nets.iter().find(|n| n.iface == iface)?;
            match what {
                "rate" => fav(format!("{iface} ↓ / ↑"), format!("{} / {}", rate(last(&n.down)), rate(last(&n.up)))),
                "total" => fav(format!("{iface} total ↓ / ↑"), format!("{} / {}", bytes(n.down_total), bytes(n.up_total))),
                _ => None,
            }
        }
        "disk" => {
            let (mount, what) = rest.rsplit_once('.')?;
            let d = m.disks.iter().find(|d| d.mount == mount)?;
            match what {
                "used" => fav(format!("Disk {mount} used"), format!("{} of {} · free {}", bytes(d.used), bytes(d.total), bytes(d.free))),
                "io" => fav(format!("Disk {mount} read / write"), format!("{} / {}", rate(last(&d.read)), rate(last(&d.write)))),
                "temp" => fav(format!("Disk {mount} temperature"), or_dash(d.temp.map(|t| format!("{t:.0} °C")))),
                _ => None,
            }
        }
        "hwmon" => {
            let s = m.hwmon.iter().find(|s| s.key() == rest)?;
            let limit = s.crit.or(s.max).map(|l| format!(" / {}", s.format(l))).unwrap_or_default();
            fav(format!("{} · {}", s.chip, s.label), format!("{}{limit}", s.format(s.value)))
        }
        "proc" => {
            // Every process of that name: survives restarts, where a pid would not.
            let list: Vec<_> = m.procs.iter().filter(|p| p.name == rest).collect();
            let label = format!("Process {rest}");
            if list.is_empty() {
                return fav(label, "not running");
            }
            let cpu: f32 = list.iter().map(|p| p.cpu).sum();
            let mem: u64 = list.iter().map(|p| p.mem).sum();
            let n = if list.len() > 1 { format!(" · {} processes", list.len()) } else { String::new() };
            fav(label, format!("{cpu:.1} % · {}{n}", bytes(mem)))
        }
        "svc" => {
            let (scope, name) = rest.split_once('.')?;
            let scope = match scope {
                "system" => Scope::System,
                "user" => Scope::User,
                _ => return None,
            };
            let label = format!("Service {name}{}", if scope == Scope::User { " (user)" } else { "" });
            let Some(u) = units.iter().find(|u| u.scope == scope && u.name == name) else {
                return fav(label, if units.is_empty() { "…" } else { "not loaded" });
            };
            let mut v = format!("{} · {}", u.active, u.sub);
            if let Some(c) = u.usage.cpu {
                v.push_str(&format!(" · {c:.1} %"));
            }
            if let Some(mm) = u.usage.mem {
                v.push_str(&format!(" · {}", bytes(mm)));
            }
            fav(label, v)
        }
        _ => None,
    }
}

/// What pages need to draw pin toggles: the view to call back and the pinned keys.
#[derive(Clone)]
pub struct Pins {
    pub view: WeakEntity<HwlensView>,
    pub set: Rc<HashSet<String>>,
}

impl Pins {
    pub fn is_pinned(&self, key: &str) -> bool {
        self.set.contains(key)
    }

    /// ☆ / ★ that toggles `key`. Shown faintly; brighter while its row (group "pin") is hovered.
    pub fn toggle(&self, key: impl Into<String>) -> Stateful<Div> {
        let key: String = key.into();
        let pinned = self.is_pinned(&key);
        let view = self.view.clone();
        let id = SharedString::from(format!("pin:{key}"));
        div()
            .id(id)
            .w(px(14.))
            .flex_shrink_0()
            .cursor_pointer()
            .text_xs()
            .text_color(if pinned { color::warn() } else { color::border() })
            .when(!pinned, |d| d.group_hover("pin", |s| s.text_color(color::muted())))
            .hover(|s| s.text_color(color::warn()))
            .child(if pinned { "★" } else { "☆" })
            .on_click(move |_, _, cx: &mut App| {
                let key = key.clone();
                let _ = view.update(cx, |v, cx| v.toggle_favorite(key, cx));
            })
    }

    /// A "label ……… value" row for a favorite key, with its pin toggle in front.
    pub fn row(&self, m: &Model, key: &str, label: &str) -> Div {
        let value = resolve(m, &[], key).map(|f| f.value).unwrap_or_else(|| "—".into());
        self.stat(key, label.to_owned(), value)
    }

    /// Like `row`, for a value the page formats itself.
    pub fn stat(&self, key: &str, label: impl Into<SharedString>, value: impl Into<SharedString>) -> Div {
        div().group("pin").flex().items_center().gap_1().child(self.toggle(key)).child(stat(label, value).flex_1())
    }

    /// A "★ Pin" / "★ Pinned" button for detail panels.
    pub fn button(&self, key: impl Into<String>) -> Stateful<Div> {
        let key: String = key.into();
        let pinned = self.is_pinned(&key);
        let view = self.view.clone();
        div()
            .id(SharedString::from(format!("pinbtn:{key}")))
            .px_2()
            .py(px(3.))
            .rounded_md()
            .border_1()
            .border_color(color::border())
            .text_xs()
            .cursor_pointer()
            .text_color(if pinned { color::warn() } else { color::muted() })
            .hover(|s| s.bg(color::hover()))
            .child(if pinned { "★ Pinned" } else { "☆ Pin to overview" })
            .on_click(move |_, _, cx: &mut App| {
                let key = key.clone();
                let _ = view.update(cx, |v, cx| v.toggle_favorite(key, cx));
            })
    }
}

impl HwlensView {
    pub(super) fn pins(&self, cx: &mut Context<Self>) -> Pins {
        Pins { view: cx.entity().downgrade(), set: Rc::new(self.favorites.iter().cloned().collect()) }
    }

    pub(super) fn toggle_favorite(&mut self, key: String, cx: &mut Context<Self>) {
        if let Some(i) = self.favorites.iter().position(|k| *k == key) {
            self.favorites.remove(i);
        } else {
            self.favorites.push(key);
        }
        if let Err(e) = save(&self.favorites) {
            self.message = Some((false, format!("favorites not saved: {e}")));
        }
        self.update_services_polling();
        cx.notify();
    }

    fn move_favorite(&mut self, i: usize, up: bool, cx: &mut Context<Self>) {
        let j = if up { i.checked_sub(1) } else { Some(i + 1).filter(|j| *j < self.favorites.len()) };
        if let Some(j) = j {
            self.favorites.swap(i, j);
            let _ = save(&self.favorites);
            cx.notify();
        }
    }

    pub(super) fn favorites_card(&self, m: &Model, cx: &mut Context<Self>) -> Div {
        let mut c = card().gap_1().child(title("Favorites", color::warn()));
        if self.favorites.is_empty() {
            return c.child(sub("Hover a value on any page and click ☆ to pin it here."));
        }
        for (i, key) in self.favorites.iter().enumerate() {
            let f = resolve(m, &self.svc.units, key);
            let (label, value) = match f {
                Some(f) => (f.label, f.value),
                None => (key.clone(), "—".into()),
            };
            let k = key.clone();
            let small = |id: (&'static str, usize), text: &'static str| {
                div().id(id).px_1().cursor_pointer().text_xs().text_color(color::border()).group_hover("fav", |s| s.text_color(color::muted())).hover(|s| s.text_color(color::text())).child(text)
            };
            c = c.child(
                div()
                    .group("fav")
                    .flex()
                    .items_center()
                    .gap_2()
                    .py(px(1.))
                    .text_xs()
                    .child(div().text_color(color::muted()).flex_shrink_0().max_w(px(260.)).truncate().child(label))
                    .child(div().flex_1())
                    .child(div().font_family(MONO).truncate().child(value))
                    .child(small(("fav-up", i), "↑").on_click(cx.listener(move |this, _, _, cx| this.move_favorite(i, true, cx))))
                    .child(small(("fav-down", i), "↓").on_click(cx.listener(move |this, _, _, cx| this.move_favorite(i, false, cx))))
                    .child(small(("fav-del", i), "✕").on_click(cx.listener(move |this, _, _, cx| this.toggle_favorite(k.clone(), cx)))),
            );
        }
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn model() -> Model {
        let snap = json!({
            "cpu": {"name": "Test CPU", "hz": "4.9 GHz", "temp": [[50.0]], "load_avg": [1.0, 2.0, 3.0], "cpu_percent": {"total": [12]}},
            "mem": {"stats": {"used": 1073741824u64, "available": 1073741824u64, "cached": 0, "free": 0}, "percent": {"used": [50]}, "swap": {"total": 0}},
            "net": {"interfaces": {"wlp.1": {"name": "wlp.1", "connected": true, "bandwidth": {"download": [1024], "upload": [0]}, "stat": {}}}, "selected": "wlp.1"},
            "proc": {"list": {
                "1": {"pid": 1, "name": "chrome", "cpu_p": 1.5, "mem": 1048576},
                "2": {"pid": 2, "name": "chrome", "cpu_p": 2.5, "mem": 1048576}
            }},
            "sys": {"threads": 100, "procs_running": 3, "procs_blocked": 1, "uptime": 60}
        });
        Model::from_snapshot(&snap, 1000)
    }

    #[test]
    fn resolves_keys() {
        let m = model();
        assert_eq!(resolve(&m, &[], "cpu.freq").unwrap().value, "4.9 GHz");
        assert_eq!(resolve(&m, &[], "sys.threads").unwrap().value, "100 / 3 / 96");
        let p = resolve(&m, &[], "proc.chrome").unwrap();
        assert_eq!(p.label, "Process chrome");
        assert!(p.value.starts_with("4.0 % · 2.0 MiB · 2 processes"), "{}", p.value);
        assert_eq!(resolve(&m, &[], "proc.gone").unwrap().value, "not running");
        assert!(resolve(&m, &[], "disk./nope.used").is_none());
        assert!(resolve(&m, &[], "bogus").is_none());
        assert_eq!(resolve(&m, &[], "svc.system.ssh.service").unwrap().value, "…");
    }
}
