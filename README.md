# hwlens

![Linux](https://img.shields.io/badge/-Linux-grey?logo=linux)
![c++23](https://img.shields.io/badge/cpp-c%2B%2B23-green)
[![Build](https://github.com/efisuby/hwlens/actions/workflows/build.yml/badge.svg)](https://github.com/efisuby/hwlens/actions/workflows/build.yml)

A terminal system monitor that focuses on hardware. It shows CPU, fans, GPU,
disks, network and processes on one screen.

hwlens has two parts: `hwlensd`, a system service that collects the data, and
the `hwlens` terminal UI, which shows it. The UI has no privileges and reads no
`/proc` or `/sys` files itself.

## Features

* **CPU**:
  * per-core load and frequency, package temperature and power (RAPL);
  * P/E core labels on hybrid CPUs, and `freq_mode = "clusters"` for
    per-cluster frequencies;
  * pressure stall information (PSI) next to the load average.
* **Fans**: speed in RPM and PWM duty in percent for each hwmon fan.
* **GPU** (NVIDIA via NVML, AMD via ROCm SMI, Intel):
  * a core and memory/IO grid with clocks, power and temperature;
  * the current throttle reason (`THRM`, `PWR`, `HW`, `APP`, `SYNC`, `IDLE`).
* **Disks**: read/write throughput (`R`/`W`), usage, and NVMe/SATA
  temperatures.
* **Network and PCIe**: download/upload (`D`/`U`) rates and graphs.
* **Processes**:
  * tree and list views, filtering and signals;
  * optional per-process VRAM and systemd unit columns.
* Themes, mouse support and a full in-app options menu.

## How it works

```
 hwlensd (root, limited capabilities)  ──  /run/hwlensd/hwlensd.sock (0660, group hwlens)  ──  hwlens (your user)
```

* `hwlensd` collects once per tick and sends every client a delta of its state
  as JSON lines ([protocol](docs/protocol.md), [design](docs/daemon.md)).
* All clients share one collection. Settings that affect collection (sensors,
  disks, update interval, ...) are sent by the UI and apply to every client.
* Signals and nice values go through the daemon. A user may act only on their
  own processes, and only root may lower a nice value. Every action is logged.
* The service runs with `CAP_DAC_READ_SEARCH CAP_SYS_PTRACE CAP_PERFMON
  CAP_KILL CAP_SYS_NICE` only, so CPU power (RAPL) and other users' process
  details work while the UI stays unprivileged.
* A planned desktop widget will read the same socket.

## Build

Requirements: Linux, CMake 3.25+, GCC 14 (or Clang 19+) and nlohmann-json 3.11+
(`apt install nlohmann-json3-dev`).

```sh
CXX=g++-14 CC=gcc-14 cmake -B build -DCMAKE_BUILD_TYPE=Release -DHWLENS_GPU=ON
cmake --build build -j"$(nproc)"
ctest --test-dir build --output-on-failure
```

To try a build without installing it, start a daemon on your own socket. As a
normal user it sees less, for example no CPU power:

```sh
./build/hwlensd --socket /tmp/hwlensd.sock &
./build/hwlens --socket /tmp/hwlensd.sock
```

Useful CMake options:

| Option | Default | Description |
|---|---|---|
| `HWLENS_GPU` | `ON` | GPU monitoring (GPU libraries are loaded at runtime) |
| `HWLENS_STATIC` | `OFF` | Link statically |
| `HWLENS_LTO` | `ON` for Release | Link-time optimisation |

## Install

On Debian/Ubuntu, build a package:

```sh
packaging/build-deb.sh
sudo apt-get install ./dist/hwlens_*.deb
```

The package installs `hwlensd.service` and `hwlensd.socket`, enables the
socket, and creates the `hwlens` group. The user who ran `sudo apt-get install`
is added to the group; log in again (or run `newgrp hwlens`) before the first
start. Add other users with `sudo usermod -aG hwlens USER`.

Without a package:

```sh
sudo cmake --install build
sudo systemd-sysusers hwlens.conf
sudo systemctl daemon-reload
sudo systemctl enable --now hwlensd.socket
```

If hwlens says that hwlensd is not running, check
`systemctl status hwlensd.socket hwlensd.service` and `journalctl -u hwlensd`.

## Configuration

Settings live in `~/.config/hwlens/hwlens.conf`. You can change them from the
options menu (`o`). User themes go in `~/.config/hwlens/themes/`.

If `~/.config/btop/btop.conf` exists on the first start, hwlens imports it
together with your btop themes. The btop files are left unchanged.

The daemon reads `/etc/hwlens/hwlensd.conf` (update interval and log level).
`hwlens --socket PATH` or `HWLENSD_SOCKET` select another socket.

## Origin

hwlens started as a fork of [btop++](https://github.com/aristocratos/btop) by
Aristocratos (forked at v1.4.8, November 2026). From 0.1.0 on, it is developed
independently and does not track upstream.

## License

[Apache License 2.0](LICENSE). See [NOTICE](NOTICE) for attribution.
