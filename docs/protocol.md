# hwlensd protocol

hwlensd listens on a unix stream socket, by default `/run/hwlensd/hwlensd.sock`
(mode `0660`, group `hwlens`). Both directions send one JSON object per line.

## Session

1. The client sends `{"hello":{"v":1}}`.
2. The daemon answers:

   ```json
   {"v":1,"seq":41,"full":{...snapshot...},"update_ms":1000}
   ```

   If the version is not supported, the daemon sends `{"error":"unsupported protocol version","v":1}`
   and closes the connection.
3. After that, the daemon sends a message after every collection (a *tick*) and after every
   request that changes the state:

   ```json
   {"seq":42,"tick":true,"update_ms":1000,"delta":{...}}
   ```

   * `seq` grows by one per message. A gap means a lost message: reconnect and say hello again.
   * `tick` is true when the delta holds new samples. A client draws one frame per tick.
     A `tick: false` message only changes derived state, for example the selected network
     interface or the details of the selected process.
   * `delta` is missing when nothing changed.

All clients share one collection. Settings and network selection that one client changes apply
to every client.

## Snapshot

`full` holds the following sections (see `src/hwlens_json.cpp`):

| Section | Contents |
|---|---|
| `v` | schema version |
| `shared` | `cores`, `total_mem` |
| `cpu` | `info` (percent histories per field and core, temperatures, load, PSI, watts, fans), `name`, `hz`, `kinds` (`P`/`E` per cpu), `sensors`, `battery`, `core_map`, ... |
| `mem` | `info` (stats, percent histories, `disks` by mount point, `disks_order`), `has_swap` |
| `net` | `ifaces` (bandwidth histories and counters per interface), `interfaces`, `selected`, `graph_max` |
| `proc` | `list` (processes keyed by pid), `detail` (selected process), `gpu_mem` (VRAM by pid) |
| `gpu` | `list`, `names`, `supported`, `count`, `shared_percent` |

Histories hold up to 600 samples.

The TUI reads only the sections above. Desktop clients also read these (`src/linux/hwlens_extra.cpp`);
a client must treat each of them as optional, since older daemons do not send them:

| Section | Contents |
|---|---|
| `sys` | `hostname`, `os` (`PRETTY_NAME`), `kernel`, `arch`, `uptime` and `boot_time` (seconds), `open_files`, `max_files`, `procs_running`, `procs_blocked`, and from the second tick on `ctxt_per_s`, `intr_per_s`, `forks_per_s` |
| `hwmon` | Every readable sensor under `/sys/class/hwmon`: `{chip, label, kind, value, max?, crit?}` with `kind` one of `temp` (°C), `fan` (rpm), `pwm` (%), `power` (W), `voltage` (V), `current` (A). Sorted by chip, kind and index; a repeated chip name gets ` #2`, ` #3`, ... |
| `proc_io` | Storage I/O per process in bytes per second from `/proc/<pid>/io`: `{"<pid>": [read, write]}`, only processes with I/O in the last tick |
| `sockets` | Refreshed every 5 ticks. `tcp_states` (count per TCP state), `udp` (count), `total`, and `list`: up to 1000 sockets of `/proc/net/{tcp,tcp6,udp,udp6}` without `TIME_WAIT`, listening first, as `{proto, local, remote, state, pid}`; `pid` is 0 when no process descriptor points at the socket |

## Deltas

A delta has the shape of the snapshot and holds only the keys that changed. Apply it like this:

* a plain value replaces the old one;
* an object is applied key by key; `{"\u0000del":1}` removes the key;
* `{"\u0000tail":[...],"\u0000size":N}` appends the values to an array and then drops the oldest
  ones until N remain (a history that got new samples);
* `{"\u0000at":{"3":{...}}}` applies a delta to element 3 of an array.

The marker keys start with a NUL character, which no data key contains.

## Requests

Requests are accepted after the hello. A client that sends settings sends them right after
the full snapshot, and the daemon answers with a `tick: false` message.

| Request | Effect |
|---|---|
| `{"options":{"update_ms":500,"show_disks":true,...}}` | Collection settings (the fields of `Shared::CollectOptions`) and the tick interval. The whole request is rejected if one field is invalid. |
| `{"net":{"select":"eth0"}}` | Select the interface shown in the net box |
| `{"net":{"rescale":true}}` | Recalculate the net graph scale |
| `{"net":{"reset_totals":true}}` | Toggle the zero point of the total counters of the selected interface |
| `{"signal":{"pid":123,"signal":15}}` | Send a signal, answered with `{"reply":"signal","ok":true,"error":""}` |
| `{"nice":{"pid":123,"nice":5}}` | Set a nice value, answered with `{"reply":"nice",...}` |
| `{"history":{"from":T,"to":T,"points":600}}` | Long term history over a time range, see below |
| `{"history":{"at":T}}` | The stored minute nearest to unix time `T`, with its busiest processes |

Errors are answered with `{"error":"..."}`. A client is disconnected after 16 errors.

### Process actions

hwlensd has `CAP_KILL` and `CAP_SYS_NICE`, so it applies the kernel's rules for unprivileged
callers itself, using the client's uid from `SO_PEERCRED`:

* a client may signal or renice only processes whose real or saved uid is its own;
* a client may raise a nice value but not lower it;
* a root client may do anything.

The daemon opens a pidfd before the checks, so a reused pid cannot redirect the signal.
Every action and every refusal is logged.

### History

hwlensd keeps one sample per minute for `history_days` days (default 7, `0` turns it off) in
`$STATE_DIRECTORY/history.jsonl` (`/var/lib/hwlensd`). A sample holds the minute's average of
each metric, the peak CPU load and the five busiest processes by CPU (average), memory (peak)
and disk I/O (average). Samples survive restarts; a minute without a running daemon has none.

| Metric | Unit |
|---|---|
| `cpu`, `cpu_max`, `iowait` | percent of all CPUs (`cpu_max`: the highest tick of the minute) |
| `mem`, `swap`, `gpu`, `vram` | percent used |
| `cpu_temp`, `gpu_temp` | °C |
| `cpu_watts`, `gpu_watts` | W |
| `psi_cpu`, `psi_mem`, `psi_io` | `some avg10`, percent |
| `load1` | 1 minute load average |
| `net_rx`, `net_tx` | bytes/s over every interface except `lo` |
| `disk_read`, `disk_write` | bytes/s over every disk |

A range request (`from` defaults to the oldest sample, `to` to now, `points` to 600, at most
3000) merges neighbouring minutes so that at most `points` remain. `step` is a whole number of
minutes. Values are averages, except `*_max`, which keep the peak. A metric missing from a merged
point is `null`.

```json
{"reply":"history","ok":true,"days":7,"oldest":1794400620,"newest":1794401220,
 "from":1794379620,"to":1794401281,"step":60,
 "t":[1794400620,1794400680],"series":{"cpu":[7.0,8.35],"mem":[31.4,31.5]}}
```

An `at` request answers `"sample":{"t":...,"v":{"cpu":...},"top":{"cpu":[[pid,"name",value],...],
"mem":[...],"io":[...]}}`, or `"sample":null` when nothing is stored. Process CPU is percent of one
core, memory is bytes, I/O is bytes/s. Errors, including a daemon with history turned off, are
answered `{"reply":"history","ok":false,"error":"..."}`.

## Limits

| Limit | Value |
|---|---|
| Request line | 64 KiB |
| Nesting depth | 8 |
| String option | 256 printable characters |
| Clients | 32 |
| Unread output per client | 32 MiB, then the client is dropped |
| `update_ms` | 100 ms to 24 h |
