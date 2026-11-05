# hwlens

![Linux](https://img.shields.io/badge/-Linux-grey?logo=linux)
![c++23](https://img.shields.io/badge/cpp-c%2B%2B23-green)
[![Build](https://github.com/efisuby/hwlens/actions/workflows/build.yml/badge.svg)](https://github.com/efisuby/hwlens/actions/workflows/build.yml)

A terminal system monitor that focuses on hardware. It shows CPU, fans, GPU,
disks, network and processes on one screen.

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

Planned: `hwlensd`, a system daemon that collects the data once and serves it
over a local socket. The TUI and a desktop widget can then share the same data.

## Build

Requirements: Linux, CMake 3.25+ and GCC 14 (or Clang 19+).

```sh
CXX=g++-14 CC=gcc-14 cmake -B build -DCMAKE_BUILD_TYPE=Release -DHWLENS_GPU=ON
cmake --build build -j"$(nproc)"
ctest --test-dir build --output-on-failure
./build/hwlens
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

The package grants `cap_perfmon,cap_dac_read_search` to `/usr/bin/hwlens`.
With these, CPU power (RAPL) and other users' process details work without
root.

Without a package: `sudo cmake --install build`.

## Configuration

Settings live in `~/.config/hwlens/hwlens.conf`. You can change them from the
options menu (`o`). User themes go in `~/.config/hwlens/themes/`.

If `~/.config/btop/btop.conf` exists on the first start, hwlens imports it
together with your btop themes. The btop files are left unchanged.

## Origin

hwlens started as a fork of [btop++](https://github.com/aristocratos/btop) by
Aristocratos (forked at v1.4.8, November 2026). From 0.1.0 on, it is developed
independently and does not track upstream.

## License

[Apache License 2.0](LICENSE). See [NOTICE](NOTICE) for attribution.
