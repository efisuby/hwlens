//! systemd services, read without hwlensd: `systemctl -o json` for units and their unit files,
//! cgroup v2 files for resources. A thread polls while the Services page is open.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use futures::channel::mpsc::UnboundedSender;
use serde_json::Value;

use crate::client::Event;

const POLL: Duration = Duration::from_secs(3);
const CGROUP: &str = "/sys/fs/cgroup";

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Scope {
    System,
    User,
}

impl Scope {
    fn args(self) -> &'static [&'static str] {
        match self {
            Scope::System => &[],
            Scope::User => &["--user"],
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Scope::System => "system",
            Scope::User => "user",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Usage {
    pub mem: Option<u64>,
    pub tasks: Option<u64>,
    /// Percent of one core since the previous poll.
    pub cpu: Option<f32>,
    /// Read + write bytes per second since the previous poll.
    pub io: Option<f32>,
}

#[derive(Clone, Debug)]
pub struct Unit {
    pub scope: Scope,
    pub name: String,
    pub load: String,
    pub active: String,
    pub sub: String,
    pub description: String,
    /// enabled, disabled, static, … from list-unit-files ("" when unknown).
    pub file_state: String,
    pub usage: Usage,
}

pub enum Update {
    Units(Vec<Unit>),
    Journal { scope: Scope, unit: String, text: String },
    Error(String),
}

#[derive(Clone)]
pub struct Services {
    active: Arc<AtomicBool>,
    wake: mpsc::Sender<()>,
    tx: UnboundedSender<Event>,
}

impl Services {
    /// Polls only while `set_active(true)`.
    pub fn start(tx: UnboundedSender<Event>) -> Services {
        let (wake, rx) = mpsc::channel();
        let active = Arc::new(AtomicBool::new(false));
        let svc = Services { active: active.clone(), wake, tx: tx.clone() };
        thread::Builder::new()
            .name("services".into())
            .spawn(move || {
                let mut prev: HashMap<PathBuf, (u64, u64, Instant)> = HashMap::new();
                loop {
                    match rx.recv_timeout(POLL) {
                        Err(mpsc::RecvTimeoutError::Disconnected) => return,
                        _ if !active.load(Ordering::Relaxed) => continue,
                        _ => {}
                    }
                    let update = match collect(&mut prev) {
                        Ok(units) => Update::Units(units),
                        Err(e) => Update::Error(e),
                    };
                    if tx.unbounded_send(Event::Services(update)).is_err() {
                        return;
                    }
                }
            })
            .expect("spawn services thread");
        svc
    }

    pub fn set_active(&self, on: bool) {
        let was = self.active.swap(on, Ordering::Relaxed);
        if on && !was {
            self.refresh();
        }
    }

    pub fn refresh(&self) {
        let _ = self.wake.send(());
    }

    /// Runs `systemctl [--user] <verb> <unit>` (polkit asks for authorization when needed).
    pub fn action(&self, scope: Scope, verb: &'static str, unit: String) {
        let svc = self.clone();
        thread::spawn(move || {
            let out = Command::new("systemctl").args(scope.args()).args([verb, "--", &unit]).output();
            let (ok, text) = match out {
                Ok(o) if o.status.success() => (true, format!("{verb} {unit}: ok")),
                Ok(o) => (false, format!("{verb} {unit}: {}", String::from_utf8_lossy(&o.stderr).trim())),
                Err(e) => (false, format!("systemctl: {e}")),
            };
            let _ = svc.tx.unbounded_send(Event::Reply { ok, text });
            svc.refresh();
        });
    }

    /// The last lines of the unit's journal.
    pub fn journal(&self, scope: Scope, unit: String) {
        let tx = self.tx.clone();
        thread::spawn(move || {
            let out = Command::new("journalctl")
                .args(scope.args())
                .args(["-u", &unit, "-n", "60", "--no-pager", "-o", "short-iso", "-q"])
                .output();
            let text = match out {
                Ok(o) => {
                    let s = String::from_utf8_lossy(&o.stdout).into_owned();
                    if s.trim().is_empty() { String::from_utf8_lossy(&o.stderr).trim().to_owned() } else { s }
                }
                Err(e) => format!("journalctl: {e}"),
            };
            let _ = tx.unbounded_send(Event::Services(Update::Journal { scope, unit, text }));
        });
    }
}

fn systemctl_json(scope: Scope, args: &[&str]) -> Result<Value, String> {
    let out = Command::new("systemctl")
        .args(scope.args())
        .args(args)
        .args(["--type=service", "-o", "json", "--no-pager"])
        .output()
        .map_err(|e| format!("systemctl: {e}"))?;
    if !out.status.success() {
        return Err(format!("systemctl {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("systemctl output: {e}"))
}

/// Units of a scope from `list-units` and `list-unit-files` output.
pub fn parse_units(scope: Scope, units: &Value, files: &Value) -> Vec<Unit> {
    let states: HashMap<&str, &str> = files
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| Some((f["unit_file"].as_str()?, f["state"].as_str()?)))
        .collect();
    units
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|u| {
            let name = u["unit"].as_str()?.to_owned();
            // Instances (getty@tty1.service) take the state of their template (getty@.service).
            let template = name.split_once('@').map(|(base, _)| format!("{base}@.service"));
            let file_state = states
                .get(name.as_str())
                .or_else(|| template.as_deref().and_then(|t| states.get(t)))
                .copied()
                .unwrap_or_default()
                .to_owned();
            let s = |k: &str| u[k].as_str().unwrap_or_default().to_owned();
            Some(Unit { scope, load: s("load"), active: s("active"), sub: s("sub"), description: s("description"), name, file_state, usage: Usage::default() })
        })
        .collect()
}

/// cgroup directories of services: (scope, unit name) -> path. User services live below
/// user@UID.service of the current user.
pub fn service_cgroups(root: &Path, uid: u32) -> HashMap<(Scope, String), PathBuf> {
    let mut found = HashMap::new();
    let mut stack = vec![(root.to_path_buf(), Scope::System, 0)];
    let manager = format!("user@{uid}.service");
    while let Some((dir, scope, depth)) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            if !e.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let name = e.file_name().to_string_lossy().into_owned();
            let path = e.path();
            if name.ends_with(".service") {
                if name == manager {
                    stack.push((path.clone(), Scope::User, depth + 1));
                }
                found.entry((scope, name)).or_insert(path);
            } else if name.ends_with(".slice") && depth < 8 {
                stack.push((path, scope, depth + 1));
            }
        }
    }
    found
}

fn read_u64(p: &Path) -> Option<u64> {
    fs::read_to_string(p).ok()?.trim().parse().ok()
}

fn cpu_usec(dir: &Path) -> Option<u64> {
    let s = fs::read_to_string(dir.join("cpu.stat")).ok()?;
    s.lines().find_map(|l| l.strip_prefix("usage_usec ")).and_then(|v| v.trim().parse().ok())
}

/// rbytes + wbytes over all devices of io.stat.
pub fn io_bytes(stat: &str) -> u64 {
    stat.split_whitespace()
        .filter_map(|kv| kv.strip_prefix("rbytes=").or_else(|| kv.strip_prefix("wbytes=")))
        .filter_map(|v| v.parse::<u64>().ok())
        .sum()
}

fn collect(prev: &mut HashMap<PathBuf, (u64, u64, Instant)>) -> Result<Vec<Unit>, String> {
    let mut units = Vec::new();
    for scope in [Scope::System, Scope::User] {
        match (systemctl_json(scope, &["list-units", "--all"]), systemctl_json(scope, &["list-unit-files"])) {
            (Ok(u), Ok(f)) => units.extend(parse_units(scope, &u, &f)),
            (Err(e), _) | (_, Err(e)) if scope == Scope::System => return Err(e),
            _ => {} // no user manager (e.g. a root session): system units only
        }
    }
    let groups = service_cgroups(Path::new(CGROUP), unsafe { libc::getuid() });
    let now = Instant::now();
    let mut seen = HashMap::new();
    for u in &mut units {
        let Some(dir) = groups.get(&(u.scope, u.name.clone())) else { continue };
        u.usage.mem = read_u64(&dir.join("memory.current"));
        u.usage.tasks = read_u64(&dir.join("pids.current"));
        let cpu = cpu_usec(dir);
        let io = fs::read_to_string(dir.join("io.stat")).ok().map(|s| io_bytes(&s));
        if let Some((pc, pio, pt)) = prev.get(dir) {
            let secs = now.duration_since(*pt).as_secs_f32().max(0.001);
            u.usage.cpu = cpu.map(|c| c.saturating_sub(*pc) as f32 / 1e6 / secs * 100.0);
            u.usage.io = io.map(|b| b.saturating_sub(*pio) as f32 / secs);
        }
        seen.insert(dir.clone(), (cpu.unwrap_or(0), io.unwrap_or(0), now));
    }
    *prev = seen;
    Ok(units)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn units_take_template_state() {
        let units = json!([
            {"unit":"ssh.service","load":"loaded","active":"active","sub":"running","description":"OpenSSH"},
            {"unit":"getty@tty1.service","load":"loaded","active":"active","sub":"running","description":"Getty"},
            {"unit":"gone.service","load":"not-found","active":"inactive","sub":"dead","description":"gone"}
        ]);
        let files = json!([
            {"unit_file":"ssh.service","state":"enabled","preset":"enabled"},
            {"unit_file":"getty@.service","state":"enabled","preset":null}
        ]);
        let u = parse_units(Scope::System, &units, &files);
        assert_eq!(u.len(), 3);
        assert_eq!(u[0].file_state, "enabled");
        assert_eq!(u[1].file_state, "enabled");
        assert_eq!(u[2].file_state, "");
        assert_eq!(u[1].sub, "running");
    }

    #[test]
    fn io_stat_sums_devices() {
        let s = "259:0 rbytes=1000 wbytes=24 rios=1 wios=2 dbytes=0 dios=0\n8:0 rbytes=6 wbytes=0 rios=0\n";
        assert_eq!(io_bytes(s), 1030);
    }

    #[test]
    fn cgroups_split_system_and_user() {
        let root = std::env::temp_dir().join(format!("hwlens-cg-{}", std::process::id()));
        for d in [
            "system.slice/ssh.service",
            "system.slice/system-getty.slice/getty@tty1.service",
            "user.slice/user-1000.slice/user@1000.service/app.slice/foo.service",
            "user.slice/user-1000.slice/user@1000.service/session.slice/pipewire.service",
            "user.slice/user-1001.slice/user@1001.service/app.slice/other.service",
        ] {
            fs::create_dir_all(root.join(d)).unwrap();
        }
        let g = service_cgroups(&root, 1000);
        fs::remove_dir_all(&root).unwrap();
        assert!(g.contains_key(&(Scope::System, "ssh.service".into())));
        assert!(g.contains_key(&(Scope::System, "getty@tty1.service".into())));
        assert!(g.contains_key(&(Scope::System, "user@1000.service".into())));
        assert!(g.contains_key(&(Scope::User, "foo.service".into())));
        assert!(g.contains_key(&(Scope::User, "pipewire.service".into())));
        assert!(!g.contains_key(&(Scope::User, "other.service".into())));
    }
}
