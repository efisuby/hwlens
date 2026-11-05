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

**1. Schema and JSON.**
* `to_json` / `from_json` for `cpu_info`, `mem_info`, `net_info`, `proc_info`, `gpu_info` and the static metadata: CPU name, core kinds, sensors.
* A `"v": 1` schema version field.
* `hwlens --dump-json` prints one snapshot.

**2. hwlensd.**
* Collection loop, configured in `/etc/hwlens/hwlensd.conf`: update interval and history length.
* Protocol, one JSON object per line:
  * `{"hello":{"v":1}}` returns the metadata and the full histories;
  * `{"subscribe":{"every":N,"sections":[...]}}` starts a stream of snapshots;
  * `{"detail":{"pid":123}}` returns extended data for one process.
* Requests are size-limited, and the request grammar is kept small.

**3. The TUI becomes a client.**
* The runner reads snapshots from the socket instead of calling `collect()`.
* `Proc::arrange()` works on the client's copy of the list.
* When the daemon is missing, the TUI prints `hwlensd is not running: systemctl start hwlensd.socket`.

**4. Actions through the daemon** (kill, signal, nice).
* The daemon gets the client uid from `SO_PEERCRED`.
* A client may act only on its own processes. A root client may act on any process.
* Every action is logged to the journal.

**5. Packaging and hardening.**
* `hwlensd.socket`: `ListenStream=/run/hwlensd/hwlensd.sock`, `SocketGroup=hwlens`, `SocketMode=0660`.
* `hwlensd.service`:
  * `CapabilityBoundingSet=CAP_DAC_READ_SEARCH CAP_SYS_PTRACE CAP_PERFMON CAP_KILL CAP_SYS_NICE`;
  * `NoNewPrivileges`, `ProtectSystem=strict`, `ProtectHome=yes`, `PrivateTmp`.
* The package creates the `hwlens` group and drops the setcap on `/usr/bin/hwlens`.

**6. Desktop widget.** A KDE Plasma plasmoid in QML with a small C++ plugin that
reads the same JSON lines through `QLocalSocket`.

The socket is `0660`, not `0666`, because the daemon exposes data that normal
users cannot read themselves, such as `/proc/<pid>/io` and `smaps` of other
users' processes.
