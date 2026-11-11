#!/bin/bash
# Build a local Ubuntu .deb of the hwlens-gui desktop client (gui/).
# Usage: packaging/build-gui-deb.sh   -> dist/hwlens-gui_<version>_<arch>.deb
set -euo pipefail

cd "$(dirname "$0")/.."
ROOT=$PWD
BUILD=$ROOT/build-gui-deb
STAGE=$BUILD/stage
DIST=$ROOT/dist

base_version=$(sed -n 's/^version = "\(.*\)"/\1/p' gui/Cargo.toml | head -1)
version="${base_version}+git$(git log -1 --format=%cd --date=format:%Y%m%d).$(git rev-list --count HEAD).$(git rev-parse --short HEAD)"
arch=$(dpkg --print-architecture)

(cd gui && cargo build --release --locked)

rm -rf "$BUILD"
install -Dm0755 gui/target/release/hwlens-gui "$STAGE/usr/bin/hwlens-gui"
strip --strip-unneeded "$STAGE/usr/bin/hwlens-gui"
install -Dm0644 gui/assets/hwlens-gui.desktop "$STAGE/usr/share/applications/hwlens-gui.desktop"
install -Dm0644 gui/assets/hwlens-gui.svg "$STAGE/usr/share/icons/hicolor/scalable/apps/hwlens-gui.svg"
install -Dm0644 LICENSE "$STAGE/usr/share/doc/hwlens-gui/copyright"
find "$STAGE" -type d -exec chmod 0755 {} +

# Linked libraries, plus the ones GPUI opens at run time (Vulkan renderer, Wayland, fonts)
mkdir -p "$BUILD/shlib/debian"
echo "Source: hwlens-gui" > "$BUILD/shlib/debian/control"
linked=$(cd "$BUILD/shlib" && dpkg-shlibdeps -O -e"$STAGE/usr/bin/hwlens-gui" 2>/dev/null | sed -n 's/^shlibs:Depends=//p')
depends="$linked, libvulkan1, mesa-vulkan-drivers | vulkan-icd, libwayland-client0, libwayland-cursor0, libfontconfig1, libfreetype6"

installed_size=$(du -sk "$STAGE" | cut -f1)
mkdir -p "$STAGE/DEBIAN"
cat > "$STAGE/DEBIAN/control" <<CONTROL
Package: hwlens-gui
Version: $version
Architecture: $arch
Maintainer: efisuby <efisuby@users.noreply.github.com>
Installed-Size: $installed_size
Depends: $depends
Recommends: hwlens
Section: utils
Priority: optional
Homepage: https://github.com/efisuby/hwlens
Description: Desktop client for the hwlensd system monitor
 A GPU-rendered window over the data hwlensd collects: CPU, memory,
 disks, network, GPUs, hardware sensors, processes and connections,
 days of per-minute history, and systemd services with their resource
 use and start/stop/enable actions.
 .
 It connects to /run/hwlensd/hwlensd.sock (package hwlens); members of
 the hwlens group may connect.
CONTROL

mkdir -p "$DIST"
dpkg-deb --build --root-owner-group "$STAGE" "$DIST/hwlens-gui_${version}_${arch}.deb"
echo "$DIST/hwlens-gui_${version}_${arch}.deb"
