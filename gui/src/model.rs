//! Typed view of a hwlensd snapshot (docs/protocol.md). Built on the client thread,
//! so the UI thread never walks JSON.

use std::collections::HashMap;

use serde_json::Value;

#[derive(Default)]
pub struct Model {
    pub update_ms: u64,
    /// The message held new samples (false: only derived state changed).
    pub tick: bool,
    pub cpu: Cpu,
    pub mem: Mem,
    /// Every interface the daemon knows, in its order.
    pub nets: Vec<Net>,
    /// The interface the daemon selected (the busiest connected one by default).
    pub net_selected: Option<String>,
    pub gpus: Vec<Gpu>,
    pub disks: Vec<Disk>,
    pub procs: Vec<Proc>,
    /// Sections only newer daemons send.
    pub sys: Option<SysInfo>,
    pub hwmon: Vec<HwSensor>,
    pub sockets: Option<Sockets>,
}

#[derive(Default)]
pub struct SysInfo {
    pub hostname: String,
    pub os: String,
    pub kernel: String,
    pub arch: String,
    pub uptime: u64,
    pub open_files: u64,
    pub max_files: u64,
    pub procs_running: u64,
    pub procs_blocked: u64,
    pub ctxt_per_s: Option<f64>,
    pub intr_per_s: Option<f64>,
    pub forks_per_s: Option<f64>,
}

pub struct HwSensor {
    pub chip: String,
    pub label: String,
    /// temp (°C), fan (rpm), pwm (%), power (W), voltage (V), current (A).
    pub kind: String,
    pub value: f32,
    pub max: Option<f32>,
    pub crit: Option<f32>,
}

impl HwSensor {
    pub fn key(&self) -> String {
        format!("{}/{}/{}", self.chip, self.kind, self.label)
    }

    pub fn unit(&self) -> &'static str {
        match self.kind.as_str() {
            "temp" => "°C",
            "fan" => "rpm",
            "pwm" => "%",
            "power" => "W",
            "voltage" => "V",
            "current" => "A",
            _ => "",
        }
    }

    pub fn format(&self, v: f32) -> String {
        match self.kind.as_str() {
            "voltage" | "current" => format!("{v:.3} {}", self.unit()),
            "power" => format!("{v:.1} {}", self.unit()),
            _ => format!("{v:.0} {}", self.unit()),
        }
    }
}

#[derive(Clone)]
pub struct Socket {
    pub proto: String,
    pub local: String,
    pub remote: String,
    pub state: String,
    pub pid: u64,
}

pub struct Sockets {
    pub list: Vec<Socket>,
    pub tcp_states: Vec<(String, u64)>,
    pub udp: u64,
    pub total: u64,
}

/// CPU time categories of `cpu.info.cpu_percent`, in stacking order.
pub const CPU_FIELDS: [&str; 8] = ["user", "nice", "system", "iowait", "irq", "softirq", "steal", "guest"];

#[derive(Default)]
pub struct Cpu {
    pub name: String,
    pub hz: String,
    /// Total usage in percent, oldest first.
    pub total: Vec<f32>,
    /// Histories of [`CPU_FIELDS`], same length as `total`.
    pub breakdown: Vec<Vec<f32>>,
    pub cores: Vec<Core>,
    /// Package (or the chosen sensor) temperature history, °C.
    pub temp_hist: Vec<f32>,
    pub temp: Option<f32>,
    pub temp_max: f32,
    pub watts: Option<f32>,
    pub load_avg: [f32; 3],
    /// Pressure stall `some avg10` in percent for cpu, memory and io.
    pub psi: Option<[f32; 3]>,
    pub fans: Vec<Fan>,
    pub battery: Option<Battery>,
    pub sensor_names: Vec<String>,
}

pub struct Core {
    pub percent: f32,
    pub hist: Vec<f32>,
    /// 'P', 'E' or ' ' when the cpu has one kind of core.
    pub kind: char,
    pub temp: Option<f32>,
}

pub struct Fan {
    pub label: String,
    pub rpm: i64,
    pub pwm: Option<f32>,
}

pub struct Battery {
    pub percent: f32,
    pub watts: f32,
    pub seconds: i64,
    pub status: String,
}

#[derive(Default)]
pub struct Mem {
    pub total: u64,
    pub used: u64,
    pub available: u64,
    pub cached: u64,
    pub free: u64,
    pub swap_total: u64,
    pub swap_used: u64,
    /// Percent histories, oldest first.
    pub used_hist: Vec<f32>,
    pub cached_hist: Vec<f32>,
    pub available_hist: Vec<f32>,
    pub swap_hist: Vec<f32>,
}

pub struct Net {
    pub iface: String,
    /// IPv4, or the MAC address when the daemon has no address.
    pub addr: String,
    pub ipv6: String,
    pub connected: bool,
    /// Bytes per second, oldest first.
    pub down: Vec<f32>,
    pub up: Vec<f32>,
    pub down_total: u64,
    pub up_total: u64,
    pub down_top: u64,
    pub up_top: u64,
}

pub struct Gpu {
    pub name: String,
    /// Percent histories, oldest first.
    pub util: Vec<f32>,
    pub vram_hist: Vec<f32>,
    pub power_hist: Vec<f32>,
    pub mem_ctrl_hist: Vec<f32>,
    pub temp_hist: Vec<f32>,
    pub temp: Option<f32>,
    pub temp_max: f32,
    pub mem_used: u64,
    pub mem_total: u64,
    /// Watts.
    pub power: Option<f32>,
    pub power_max: Option<f32>,
    pub clock_mhz: Option<u64>,
    pub mem_clock_mhz: Option<u64>,
    /// PCIe throughput, bytes per second.
    pub pcie_rx: Option<u64>,
    pub pcie_tx: Option<u64>,
    pub encoder: Option<u64>,
    pub decoder: Option<u64>,
    pub fan: Option<u64>,
    pub pstate: Option<u64>,
    pub throttle: Vec<&'static str>,
}

pub struct Disk {
    pub mount: String,
    pub name: String,
    pub dev: String,
    pub fstype: String,
    pub used: u64,
    pub free: u64,
    pub total: u64,
    pub used_percent: f32,
    /// Bytes per second, oldest first.
    pub read: Vec<f32>,
    pub write: Vec<f32>,
    /// Busy time in percent.
    pub activity: Vec<f32>,
    pub temp: Option<f32>,
    pub is_swap: bool,
}

#[derive(Clone)]
pub struct Proc {
    pub pid: u64,
    pub ppid: u64,
    pub name: String,
    pub cmd: String,
    pub user: String,
    pub unit: String,
    pub state: char,
    pub cpu: f32,
    pub mem: u64,
    pub vram: Option<u64>,
    /// Storage I/O in bytes per second (newer daemons).
    pub io_read: f32,
    pub io_write: f32,
    pub threads: u64,
    pub nice: i64,
}

fn f32s(v: &Value) -> Vec<f32> {
    v.as_array()
        .map(|a| a.iter().filter_map(Value::as_f64).map(|x| x as f32).collect())
        .unwrap_or_default()
}

fn u64_of(v: &Value) -> u64 {
    v.as_u64().or_else(|| v.as_f64().map(|f| f.max(0.0) as u64)).unwrap_or(0)
}

fn pos(v: &Value) -> Option<u64> {
    v.as_u64().filter(|&x| x > 0)
}

fn str_of(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_owned()
}

fn last(v: &Value) -> Option<f32> {
    v.as_array()?.last()?.as_f64().map(|x| x as f32)
}

impl Model {
    pub fn from_snapshot(s: &Value, update_ms: u64) -> Self {
        let gpu_mem: HashMap<u64, u64> = s["proc"]["gpu_mem"]
            .as_object()
            .map(|m| m.iter().filter_map(|(k, v)| Some((k.parse().ok()?, u64_of(v)))).collect())
            .unwrap_or_default();
        let secs = update_ms.max(1) as f32 / 1000.0;
        Model {
            update_ms,
            tick: true,
            cpu: cpu(&s["cpu"]),
            mem: mem(s),
            nets: nets(&s["net"]),
            net_selected: s["net"]["selected"].as_str().map(str::to_owned),
            gpus: gpus(&s["gpu"]),
            disks: disks(&s["mem"]["info"], secs),
            procs: procs(&s["proc"]["list"], &gpu_mem, &s["proc_io"]),
            sys: sys(&s["sys"]),
            hwmon: hwmon(&s["hwmon"]),
            sockets: sockets(&s["sockets"]),
        }
    }

    pub fn net(&self) -> Option<&Net> {
        let sel = self.net_selected.as_deref()?;
        self.nets.iter().find(|n| n.iface == sel)
    }
}

fn cpu(c: &Value) -> Cpu {
    let info = &c["info"];
    let kinds: Vec<char> = c["kinds"].as_str().unwrap_or_default().chars().collect();
    let hybrid = kinds.contains(&'P') && kinds.contains(&'E');
    let temps = info["temp"].as_array().cloned().unwrap_or_default();
    let cores = info["core_percent"]
        .as_array()
        .map(|a| {
            a.iter()
                .enumerate()
                .map(|(i, h)| {
                    // temp[0] is the package; temp[i + 1] is cpu i (see Cpu::core_mapping).
                    let t = temps.get(i + 1);
                    Core {
                        percent: last(h).unwrap_or(0.0),
                        hist: f32s(h),
                        kind: if hybrid { kinds.get(i).copied().unwrap_or(' ') } else { ' ' },
                        temp: t.and_then(last),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let la = f32s(&info["load_avg"]);
    let psi = f32s(&info["pressure"]);
    let fans = info["fans"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|f| Fan {
                    label: str_of(&f["label"]),
                    rpm: f["rpm"].as_i64().unwrap_or(0),
                    pwm: f["pwm_percent"].as_f64().filter(|&p| p >= 0.0).map(|p| p as f32),
                })
                .collect()
        })
        .unwrap_or_default();
    let bat = &c["battery"];
    let battery = c["has_battery"].as_bool().unwrap_or(false).then(|| Battery {
        percent: bat[0].as_f64().unwrap_or(0.0) as f32,
        watts: bat[1].as_f64().unwrap_or(0.0) as f32,
        seconds: bat[2].as_i64().unwrap_or(0),
        status: str_of(&bat[3]),
    });
    let total = f32s(&info["cpu_percent"]["total"]);
    let breakdown = CPU_FIELDS.iter().map(|f| f32s(&info["cpu_percent"][*f])).collect();
    Cpu {
        name: str_of(&c["name"]),
        hz: str_of(&c["hz"]),
        total,
        breakdown,
        cores,
        temp_hist: temps.first().map(f32s).unwrap_or_default(),
        temp: temps.first().and_then(last),
        temp_max: info["temp_max"].as_f64().filter(|&t| t > 0.0).unwrap_or(100.0) as f32,
        watts: c["watts"]
            .as_bool()
            .unwrap_or(false)
            .then(|| info["usage_watts"].as_f64().map(|w| w as f32))
            .flatten(),
        load_avg: [0, 1, 2].map(|i| la.get(i).copied().unwrap_or(0.0)),
        psi: (psi.len() == 3 && psi.iter().all(|&p| p >= 0.0)).then(|| [psi[0], psi[1], psi[2]]),
        fans,
        battery,
        sensor_names: c["sensors"]
            .as_array()
            .map(|a| a.iter().map(str_of).filter(|s| s != "Auto").collect())
            .unwrap_or_default(),
    }
}

fn mem(s: &Value) -> Mem {
    let st = &s["mem"]["info"]["stats"];
    let pc = &s["mem"]["info"]["percent"];
    Mem {
        total: u64_of(&s["shared"]["total_mem"]),
        used: u64_of(&st["used"]),
        available: u64_of(&st["available"]),
        cached: u64_of(&st["cached"]),
        free: u64_of(&st["free"]),
        swap_total: u64_of(&st["swap_total"]),
        swap_used: u64_of(&st["swap_used"]),
        used_hist: f32s(&pc["used"]),
        cached_hist: f32s(&pc["cached"]),
        available_hist: f32s(&pc["available"]),
        swap_hist: f32s(&pc["swap_used"]),
    }
}

fn nets(n: &Value) -> Vec<Net> {
    let order: Vec<String> = n["interfaces"].as_array().map(|a| a.iter().map(str_of).collect()).unwrap_or_default();
    order
        .into_iter()
        .filter_map(|iface| {
            let i = &n["ifaces"][&iface];
            (!i.is_null()).then(|| Net {
                addr: str_of(&i["ipv4"]),
                ipv6: str_of(&i["ipv6"]),
                connected: i["connected"].as_bool().unwrap_or(false),
                down: f32s(&i["bandwidth"]["download"]),
                up: f32s(&i["bandwidth"]["upload"]),
                down_total: u64_of(&i["stat"]["download"]["total"]),
                up_total: u64_of(&i["stat"]["upload"]["total"]),
                down_top: u64_of(&i["stat"]["download"]["top"]),
                up_top: u64_of(&i["stat"]["upload"]["top"]),
                iface,
            })
        })
        .collect()
}

/// NVML clocks event reasons (nvmlClocksEventReason*), most interesting first.
const THROTTLE: [(u64, &str); 8] = [
    (0x40, "HW thermal"),
    (0x20, "SW thermal"),
    (0x08, "HW slowdown"),
    (0x80, "power brake"),
    (0x04, "power cap"),
    (0x10, "sync boost"),
    (0x02, "app clocks"),
    (0x100, "display clock"),
];

fn gpus(g: &Value) -> Vec<Gpu> {
    let names = g["names"].as_array().cloned().unwrap_or_default();
    g["list"]
        .as_array()
        .map(|list| {
            list.iter()
                .enumerate()
                .map(|(i, d)| {
                    let reasons = d["clocks_event_reasons"].as_u64().unwrap_or(0);
                    Gpu {
                        name: names.get(i).map(str_of).unwrap_or_else(|| format!("GPU {i}")),
                        util: f32s(&d["gpu_percent"]["gpu-totals"]),
                        vram_hist: f32s(&d["gpu_percent"]["gpu-vram-totals"]),
                        power_hist: f32s(&d["gpu_percent"]["gpu-pwr-totals"]),
                        mem_ctrl_hist: f32s(&d["mem_utilization_percent"]),
                        temp_hist: f32s(&d["temp"]),
                        temp: last(&d["temp"]),
                        temp_max: d["temp_max"].as_f64().filter(|&t| t > 0.0).unwrap_or(100.0) as f32,
                        mem_used: u64_of(&d["mem_used"]),
                        mem_total: u64_of(&d["mem_total"]),
                        // NVML reports milliwatts.
                        power: d["pwr_usage"].as_f64().filter(|&p| p > 0.0).map(|p| (p / 1000.0) as f32),
                        power_max: d["pwr_max_usage"].as_f64().filter(|&p| p > 0.0).map(|p| (p / 1000.0) as f32),
                        clock_mhz: pos(&d["gpu_clock_speed"]),
                        mem_clock_mhz: pos(&d["mem_clock_speed"]),
                        // NVML reports KiB/s.
                        pcie_rx: d["pcie_rx"].as_u64().map(|k| k * 1024),
                        pcie_tx: d["pcie_tx"].as_u64().map(|k| k * 1024),
                        encoder: d["encoder_utilization"].as_u64(),
                        decoder: d["decoder_utilization"].as_u64(),
                        fan: d["fan_speed"].as_u64(),
                        pstate: d["pwr_state"].as_u64().filter(|&p| p < 32),
                        throttle: THROTTLE.iter().filter(|(bit, _)| reasons & bit != 0).map(|(_, n)| *n).collect(),
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

fn disks(info: &Value, secs: f32) -> Vec<Disk> {
    let order = info["disks_order"].as_array().cloned().unwrap_or_default();
    // io_read/io_write hold bytes per collection, io_activity a percentage.
    let per_sec = |v: &Value| f32s(v).into_iter().map(|b| b / secs).collect();
    order
        .iter()
        .filter_map(Value::as_str)
        .filter_map(|m| {
            let d = &info["disks"][m];
            (!d.is_null()).then(|| Disk {
                mount: m.to_owned(),
                name: str_of(&d["name"]),
                dev: str_of(&d["dev"]),
                fstype: str_of(&d["fstype"]),
                used: u64_of(&d["used"]),
                free: u64_of(&d["free"]),
                total: u64_of(&d["total"]),
                used_percent: d["used_percent"].as_f64().unwrap_or(0.0) as f32,
                read: per_sec(&d["io_read"]),
                write: per_sec(&d["io_write"]),
                activity: f32s(&d["io_activity"]),
                temp: d["temp"].as_f64().filter(|&t| t > 0.0).map(|t| t as f32),
                is_swap: m == "swap",
            })
        })
        .collect()
}

fn sys(v: &Value) -> Option<SysInfo> {
    v.as_object()?;
    Some(SysInfo {
        hostname: str_of(&v["hostname"]),
        os: str_of(&v["os"]),
        kernel: str_of(&v["kernel"]),
        arch: str_of(&v["arch"]),
        uptime: u64_of(&v["uptime"]),
        open_files: u64_of(&v["open_files"]),
        max_files: u64_of(&v["max_files"]),
        procs_running: u64_of(&v["procs_running"]),
        procs_blocked: u64_of(&v["procs_blocked"]),
        ctxt_per_s: v["ctxt_per_s"].as_f64(),
        intr_per_s: v["intr_per_s"].as_f64(),
        forks_per_s: v["forks_per_s"].as_f64(),
    })
}

fn hwmon(v: &Value) -> Vec<HwSensor> {
    let opt = |x: &Value| x.as_f64().map(|f| f as f32);
    v.as_array()
        .map(|a| {
            a.iter()
                .map(|s| HwSensor {
                    chip: str_of(&s["chip"]),
                    label: str_of(&s["label"]),
                    kind: str_of(&s["kind"]),
                    value: s["value"].as_f64().unwrap_or(0.0) as f32,
                    max: opt(&s["max"]),
                    crit: opt(&s["crit"]),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn sockets(v: &Value) -> Option<Sockets> {
    v.as_object()?;
    let mut tcp_states: Vec<(String, u64)> = v["tcp_states"]
        .as_object()
        .map(|m| m.iter().map(|(k, n)| (k.clone(), u64_of(n))).collect())
        .unwrap_or_default();
    tcp_states.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    Some(Sockets {
        list: v["list"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|s| Socket {
                        proto: str_of(&s["proto"]),
                        local: str_of(&s["local"]),
                        remote: str_of(&s["remote"]),
                        state: str_of(&s["state"]),
                        pid: u64_of(&s["pid"]),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        tcp_states,
        udp: u64_of(&v["udp"]),
        total: u64_of(&v["total"]),
    })
}

fn procs(list: &Value, gpu_mem: &HashMap<u64, u64>, io: &Value) -> Vec<Proc> {
    let Some(map) = list.as_object() else { return Vec::new() };
    map.values()
        .map(|p| {
            let pid = u64_of(&p["pid"]);
            let rw = &io[pid.to_string()];
            Proc {
                pid,
                ppid: u64_of(&p["ppid"]),
                name: str_of(&p["name"]),
                cmd: str_of(&p["cmd"]),
                user: str_of(&p["user"]),
                unit: str_of(&p["unit"]),
                state: p["state"].as_str().and_then(|s| s.chars().next()).unwrap_or('?'),
                cpu: p["cpu_p"].as_f64().unwrap_or(0.0) as f32,
                mem: u64_of(&p["mem"]),
                vram: gpu_mem.get(&pid).copied(),
                io_read: rw[0].as_f64().unwrap_or(0.0) as f32,
                io_write: rw[1].as_f64().unwrap_or(0.0) as f32,
                threads: u64_of(&p["threads"]),
                nice: p["nice"].as_i64().unwrap_or(0),
            }
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

pub fn duration(secs: i64) -> String {
    let (d, h, m) = (secs / 86400, secs / 3600 % 24, secs / 60 % 60);
    if d > 0 { format!("{d}d {h}h") } else if h > 0 { format!("{h}h {m:02}m") } else { format!("{m}m {:02}s", secs % 60) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn core_temps_follow_package_offset() {
        let s = json!({"cpu": {"kinds": "PE", "info": {
            "core_percent": [[1, 2], [3, 4]],
            "temp": [[50, 51], [40, 41], [60, 61]],
            "cpu_percent": {"total": [5, 6], "user": [3, 4]}
        }}});
        let m = Model::from_snapshot(&s, 1000);
        assert_eq!(m.cpu.temp, Some(51.0));
        assert_eq!(m.cpu.cores[0].temp, Some(41.0));
        assert_eq!(m.cpu.cores[1].temp, Some(61.0));
        assert_eq!((m.cpu.cores[0].kind, m.cpu.cores[1].kind), ('P', 'E'));
        assert_eq!(m.cpu.breakdown[0], vec![3.0, 4.0]);
    }

    #[test]
    fn disk_io_is_per_second() {
        let s = json!({"mem": {"info": {"disks_order": ["/"], "disks": {"/": {"io_read": [1000, 2000], "io_write": [0, 500]}}}}});
        let m = Model::from_snapshot(&s, 500);
        assert_eq!(m.disks[0].read, vec![2000.0, 4000.0]);
        assert_eq!(m.disks[0].write, vec![0.0, 1000.0]);
    }

    #[test]
    fn extra_sections_parse() {
        let s = json!({
            "sys": {"hostname": "box", "uptime": 90061, "ctxt_per_s": 1500.0},
            "hwmon": [{"chip": "nct", "label": "in0", "kind": "voltage", "value": 1.152}],
            "sockets": {"list": [{"proto": "tcp", "local": "0.0.0.0:22", "remote": "0.0.0.0:0", "state": "LISTEN", "pid": 7}],
                        "tcp_states": {"LISTEN": 1, "ESTABLISHED": 5}, "udp": 2, "total": 8},
            "proc_io": {"7": [100.0, 2000.0]},
            "proc": {"list": {"7": {"pid": 7}, "8": {"pid": 8}}}
        });
        let m = Model::from_snapshot(&s, 1000);
        let sys = m.sys.unwrap();
        assert_eq!((sys.hostname.as_str(), sys.uptime, sys.ctxt_per_s), ("box", 90061, Some(1500.0)));
        assert_eq!(m.hwmon[0].format(m.hwmon[0].value), "1.152 V");
        let so = m.sockets.unwrap();
        assert_eq!(so.tcp_states[0], ("ESTABLISHED".to_string(), 5));
        assert_eq!(so.list[0].pid, 7);
        let p7 = m.procs.iter().find(|p| p.pid == 7).unwrap();
        let p8 = m.procs.iter().find(|p| p.pid == 8).unwrap();
        assert_eq!((p7.io_read, p7.io_write, p8.io_write), (100.0, 2000.0, 0.0));
    }

    #[test]
    fn old_daemon_has_no_extra_sections() {
        let m = Model::from_snapshot(&json!({"cpu": {}}), 1000);
        assert!(m.sys.is_none() && m.hwmon.is_empty() && m.sockets.is_none());
    }

    #[test]
    fn vram_joins_by_pid() {
        let s = json!({"proc": {"gpu_mem": {"7": 4096}, "list": {"7": {"pid": 7, "name": "a"}, "8": {"pid": 8}}}});
        let m = Model::from_snapshot(&s, 1000);
        let p7 = m.procs.iter().find(|p| p.pid == 7).unwrap();
        let p8 = m.procs.iter().find(|p| p.pid == 8).unwrap();
        assert_eq!((p7.vram, p8.vram), (Some(4096), None));
    }
}
