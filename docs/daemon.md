# hwlensd: daemon and client

hwlens is being split into a system daemon that collects the data and clients
that show it. The TUI stays the main client. A desktop widget comes later.

```
            root (limited capabilities)                       user (group hwlens)
 ┌─────────────── hwlensd ───────────────┐  /run/hwlensd/hwlensd.sock  ┌─── hwlens (TUI) ───┐
 │ tick → Cpu/Gpu/Mem/Net/Proc::collect  │  0660 root:hwlens           │ no privileges,     │
 │ fixed-length histories                │ ◀──────── requests ──────── │ no collectors      │
 │ JSON snapshot per tick                │ ───────── JSON lines ─────▶ │ sort/filter/tree   │
 │ SO_PEERCRED: client uid               │                             │ on the client      │
 └───────────────────────────────────────┘                             └────────────────────┘
```

Decisions:
* hwlens does not work without the daemon.
* The daemon is a system service and runs as root.
* Messages are JSON lines.

## Stages

**0. Split collection from the UI.** *Done.*
* Collectors read `Shared::CollectOptions` instead of `Config`.
* `Proc::arrange()` builds the filter, sort and tree outside `Proc::collect()`.
* Histories have a fixed length (`Shared::history_size`).
* Linux only.

**1. Schema and JSON.** *Done.*
* `src/hwlens_json.cpp`: snapshot of every collect() result and the collector globals
  (CPU name, core kinds, sensors, selected interface, ...), with a `"v": 1` schema version.
* `diff()` / `patch()` turn two snapshots into a delta; histories that got new samples are sent as their tail.
* `hwlensd --dump-json` prints one snapshot.

**2. hwlensd.** *Done.*
* Collection loop; `/etc/hwlens/hwlensd.conf` sets the update interval and the log level.
* Protocol in [protocol.md](protocol.md): `hello` returns the full snapshot, then every tick sends a delta.
  Settings (`options`), net box actions, signals and nice values are requests.
* Requests are size- and depth-limited, and every option is validated before it is applied.

**3. The TUI becomes a client.** *Done.*
* `src/hwlens_client.cpp` keeps a copy of the daemon's snapshot and serves `Cpu/Mem/Net/Proc/Gpu::collect()`
  from it, so the drawing code did not change. `hwlens` no longer links the collectors.
* The runner loads the newest state at the start of a cycle and sends the settings when they change.
  The main loop draws a frame per tick of the daemon.
* `Proc::arrange()` works on the client's copy of the list; collapsed tree branches stay on the client.
* Without the daemon, the TUI exits with
  `hwlensd is not running (...). Start it with: sudo systemctl enable --now hwlensd.socket`.
  A lost connection is retried every second, and the settings are sent again.

**4. Actions through the daemon** (kill, signal, nice). *Done.*
* The daemon gets the client uid from `SO_PEERCRED` and pins the process with a pidfd.
* A client may act only on processes whose real or saved uid is its own, and may not lower a nice value.
  A root client may do anything.
* Every action and every refusal is logged.

**5. Packaging and hardening.** *Done.*
* `hwlensd.socket`: `ListenStream=/run/hwlensd/hwlensd.sock`, `SocketGroup=hwlens`, `SocketMode=0660`.
* `hwlensd.service`:
  * `CapabilityBoundingSet=CAP_DAC_READ_SEARCH CAP_SYS_PTRACE CAP_PERFMON CAP_KILL CAP_SYS_NICE`;
  * `NoNewPrivileges`, `ProtectSystem=strict`, `ProtectHome=yes`, `PrivateTmp`.
* `ProtectHome=read-only` rather than `yes`, so that `statvfs()` of a separate `/home` sees the real file system.
* The package creates the `hwlens` group (sysusers.d), adds the installing sudo user to it,
  enables the socket and drops the setcap on `/usr/bin/hwlens`.
* `systemd-analyze security hwlensd.service`: exposure 4.6 (OK).

**6a. Desktop client.** *First version.*
* `gui/` is `hwlens-gui`, a separate Rust program built with GPUI (the UI toolkit of Zed).
  It speaks the same protocol: `gui/src/patch.rs` mirrors `patch()` and has unit tests.
* Summary cards with graphs (CPU, memory, network, first GPU), per-core load with P/E kinds,
  disks, and a process table with sorting, a type-to-filter search and SIGTERM/SIGKILL.
* `cd gui && cargo build --release`; needs `libxkbcommon-x11-dev`, `libwayland-dev`, `libvulkan-dev`.
* `vendor/xattr` patches `xattr 0.2.3` (pulled in by gpui), which uses a constant current libc dropped.
* Pages: Overview, CPU, Memory, Disks, Network, GPU, Processes (tree, details, signals, nice),
  Connections and Sensors (`--page NAME` opens one; Ctrl+1…9 switch).
* It never sends `options`: they are shared by every client and would change what the TUI shows.
  Per-process and per-sensor history is kept on the client instead.
* hwlensd sends extra sections for it (`sys`, `hwmon`, `proc_io`, `sockets`, see protocol.md),
  collected by `Extra::Collector` in `src/linux/hwlens_extra.cpp`. The delta grows by about 7 %.

**6. Desktop widget.** A KDE Plasma plasmoid in QML with a small C++ plugin that
reads the same JSON lines through `QLocalSocket`.

The socket is `0660`, not `0666`, because the daemon exposes data that normal
users cannot read themselves, such as `/proc/<pid>/io` and `smaps` of other
users' processes.
