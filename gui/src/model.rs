//! Typed view of a hwlensd snapshot (docs/protocol.md). Built on the client thread,
//! so the UI thread never walks JSON.

use serde_json::Value;

#[derive(Default)]
pub struct Model {
    pub update_ms: u64,
    pub cpu: Cpu,
    pub mem: Mem,
    pub net: Option<Net>,
    pub gpus: Vec<Gpu>,
    pub disks: Vec<Disk>,
    pub procs: Vec<Proc>,
}

#[derive(Default)]
pub struct Cpu {
    pub name: String,
    pub hz: String,
    /// Total usage in percent, oldest first.
    pub total: Vec<f32>,
    pub cores: Vec<Core>,
    pub temp: Option<f32>,
    pub watts: Option<f32>,
    pub load_avg: [f32; 3],
}

pub struct Core {
    pub percent: f32,
    /// 'P', 'E' or ' ' when the cpu has one kind of core.
    pub kind: char,
}

#[derive(Default)]
pub struct Mem {
    pub total: u64,
    pub used: u64,
    pub cached: u64,
    pub swap_total: u64,
    pub swap_used: u64,
    /// Used memory in percent, oldest first.
    pub used_hist: Vec<f32>,
}

pub struct Net {
    pub iface: String,
    pub ip: String,
    /// Bytes per second, oldest first.
    pub down: Vec<f32>,
    pub up: Vec<f32>,
}

pub struct Gpu {
    pub name: String,
    /// Utilization in percent, oldest first.
    pub util: Vec<f32>,
    pub temp: Option<f32>,
    pub mem_used: u64,
    pub mem_total: u64,
    /// Watts.
    pub power: Option<f32>,
    pub clock_mhz: Option<u64>,
}

pub struct Disk {
    pub mount: String,
    pub fstype: String,
    pub used: u64,
    pub total: u64,
    pub used_percent: f32,
}

#[derive(Clone)]
pub struct Proc {
    pub pid: u64,
    pub name: String,
    pub cmd: String,
    pub user: String,
    pub state: char,
    pub cpu: f32,
    pub mem: u64,
    pub threads: u64,
}

fn f32s(v: &Value) -> Vec<f32> {
    v.as_array()
        .map(|a| a.iter().filter_map(Value::as_f64).map(|x| x as f32).collect())
        .unwrap_or_default()
}

fn u64_of(v: &Value) -> u64 {
    v.as_u64().or_else(|| v.as_f64().map(|f| f.max(0.0) as u64)).unwrap_or(0)
}

fn str_of(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_owned()
}

fn last(v: &Value) -> Option<f32> {
    v.as_array()?.last()?.as_f64().map(|x| x as f32)
}

impl Model {
    pub fn from_snapshot(s: &Value, update_ms: u64) -> Self {
        Model {
            update_ms,
            cpu: cpu(&s["cpu"]),
            mem: mem(s),
            net: net(&s["net"]),
            gpus: gpus(&s["gpu"]),
            disks: disks(&s["mem"]["info"]),
            procs: procs(&s["proc"]["list"]),
        }
    }
}

fn cpu(c: &Value) -> Cpu {
    let info = &c["info"];
    let kinds: Vec<char> = c["kinds"].as_str().unwrap_or_default().chars().collect();
    let hybrid = kinds.contains(&'P') && kinds.contains(&'E');
    let cores = info["core_percent"]
        .as_array()
        .map(|a| {
            a.iter()
                .enumerate()
                .map(|(i, h)| Core {
                    percent: last(h).unwrap_or(0.0),
                    kind: if hybrid { kinds.get(i).copied().unwrap_or(' ') } else { ' ' },
                })
                .collect()
        })
        .unwrap_or_default();
    let la = f32s(&info["load_avg"]);
    Cpu {
        name: str_of(&c["name"]),
        hz: str_of(&c["hz"]),
        total: f32s(&info["cpu_percent"]["total"]),
        cores,
        // temp[0] is the package (or the first sensor), the rest follow core_map.
        temp: info["temp"].get(0).and_then(last),
        watts: c["watts"]
            .as_bool()
            .unwrap_or(false)
            .then(|| info["usage_watts"].as_f64().map(|w| w as f32))
            .flatten(),
        load_avg: [0, 1, 2].map(|i| la.get(i).copied().unwrap_or(0.0)),
    }
}

fn mem(s: &Value) -> Mem {
    let st = &s["mem"]["info"]["stats"];
    Mem {
        total: u64_of(&s["shared"]["total_mem"]),
        used: u64_of(&st["used"]),
        cached: u64_of(&st["cached"]),
        swap_total: u64_of(&st["swap_total"]),
        swap_used: u64_of(&st["swap_used"]),
        used_hist: f32s(&s["mem"]["info"]["percent"]["used"]),
    }
}

fn net(n: &Value) -> Option<Net> {
    let iface = n["selected"].as_str()?.to_owned();
    let i = &n["ifaces"][&iface];
    if i.is_null() {
        return None;
    }
    Some(Net {
        ip: str_of(&i["ipv4"]),
        down: f32s(&i["bandwidth"]["download"]),
        up: f32s(&i["bandwidth"]["upload"]),
        iface,
    })
}

fn gpus(g: &Value) -> Vec<Gpu> {
    let names = g["names"].as_array().cloned().unwrap_or_default();
    g["list"]
        .as_array()
        .map(|list| {
            list.iter()
                .enumerate()
                .map(|(i, d)| Gpu {
                    name: names.get(i).map(str_of).unwrap_or_else(|| format!("GPU {i}")),
                    util: f32s(&d["gpu_percent"]["gpu-totals"]),
                    temp: last(&d["temp"]),
                    mem_used: u64_of(&d["mem_used"]),
                    mem_total: u64_of(&d["mem_total"]),
                    // NVML reports milliwatts.
                    power: d["pwr_usage"].as_f64().filter(|&p| p > 0.0).map(|p| (p / 1000.0) as f32),
                    clock_mhz: d["gpu_clock_speed"].as_u64().filter(|&c| c > 0),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn disks(info: &Value) -> Vec<Disk> {
    let order = info["disks_order"].as_array().cloned().unwrap_or_default();
    order
        .iter()
        .filter_map(Value::as_str)
        .filter(|m| *m != "swap")
        .filter_map(|m| {
            let d = &info["disks"][m];
            (!d.is_null()).then(|| Disk {
                mount: m.to_owned(),
                fstype: str_of(&d["fstype"]),
                used: u64_of(&d["used"]),
                total: u64_of(&d["total"]),
                used_percent: d["used_percent"].as_f64().unwrap_or(0.0) as f32,
            })
        })
        .collect()
}

fn procs(list: &Value) -> Vec<Proc> {
    let Some(map) = list.as_object() else { return Vec::new() };
    map.values()
        .map(|p| Proc {
            pid: u64_of(&p["pid"]),
            name: str_of(&p["name"]),
            cmd: str_of(&p["cmd"]),
            user: str_of(&p["user"]),
            state: p["state"].as_str().and_then(|s| s.chars().next()).unwrap_or('?'),
            cpu: p["cpu_p"].as_f64().unwrap_or(0.0) as f32,
            mem: u64_of(&p["mem"]),
            threads: u64_of(&p["threads"]),
        })
        .collect()
}

pub fn bytes(n: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u + 1 < UNITS.len() {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 { format!("{n} B") } else if v >= 100.0 { format!("{v:.0} {}", UNITS[u]) } else { format!("{v:.1} {}", UNITS[u]) }
}

pub fn rate(bps: f32) -> String {
    format!("{}/s", bytes(bps.max(0.0) as u64))
}
