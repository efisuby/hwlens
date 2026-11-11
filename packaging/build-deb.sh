#!/bin/bash
# Build a local Ubuntu .deb of this hwlens checkout.
# Usage: packaging/build-deb.sh   -> dist/hwlens_<version>_<arch>.deb
set -euo pipefail

cd "$(dirname "$0")/.."
ROOT=$PWD
BUILD=$ROOT/build-deb
STAGE=$BUILD/stage
DIST=$ROOT/dist

: "${CXX:=g++-14}" "${CC:=gcc-14}"
export CXX CC

base_version=$(grep -zoP 'project\(\s*"?hwlens"?\s+VERSION\s+\K[0-9.]+' CMakeLists.txt | tr -d '\0' || true)
[[ -n "$base_version" ]] || base_version=$(git describe --tags --abbrev=0 | sed 's/^v//')
version="${base_version}+git$(git log -1 --format=%cd --date=format:%Y%m%d).$(git rev-list --count HEAD).$(git rev-parse --short HEAD)"
arch=$(dpkg --print-architecture)

rm -rf "$BUILD"
cmake -B "$BUILD" -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX=/usr -DHWLENS_GPU=ON
cmake --build "$BUILD" -j"$(nproc)"
DESTDIR="$STAGE" cmake --install "$BUILD"
# The packaged permissions are the installed ones, whatever the umask of the build was
find "$STAGE" -type d -exec chmod 0755 {} +

# Runtime dependencies from the linked shared libraries
mkdir -p "$BUILD/shlib/debian"
echo "Source: hwlens" > "$BUILD/shlib/debian/control"
depends=$(cd "$BUILD/shlib" && dpkg-shlibdeps -O -e"$STAGE/usr/bin/hwlens" -e"$STAGE/usr/bin/hwlensd" 2>/dev/null | sed -n 's/^shlibs:Depends=//p')

installed_size=$(du -sk "$STAGE" | cut -f1)
mkdir -p "$STAGE/DEBIAN"
cat > "$STAGE/DEBIAN/control" <<CONTROL
Package: hwlens
Version: $version
Architecture: $arch
Maintainer: efisuby <efisuby@users.noreply.github.com>
Installed-Size: $installed_size
Depends: $depends, systemd
Section: utils
Priority: optional
Homepage: https://github.com/efisuby/hwlens
Description: Terminal system monitor focused on hardware
 Shows CPU (P/E clusters, RAPL power, PSI), fan speeds and PWM, GPU
 clocks and throttle reasons, disk IO and NVMe temperatures, network
 and processes with per-process VRAM and systemd unit columns.
 .
 The hwlensd system service collects the data; the hwlens terminal UI
 reads it from /run/hwlensd/hwlensd.sock. Members of the hwlens group
 may connect.
CONTROL

echo /etc/hwlens/hwlensd.conf > "$STAGE/DEBIAN/conffiles"

cat > "$STAGE/DEBIAN/postinst" <<'POSTINST'
#!/bin/sh
set -e
if [ "$1" = configure ]; then
	systemd-sysusers hwlens.conf 2>/dev/null || getent group hwlens >/dev/null || addgroup --system hwlens
	# The user who ran "sudo apt install" gets access to the daemon; other users are added by hand
	if [ -n "${SUDO_USER:-}" ] && [ "$SUDO_USER" != root ] && id "$SUDO_USER" >/dev/null 2>&1 \
		&& ! id -nG "$SUDO_USER" | tr ' ' '\n' | grep -qx hwlens; then
		usermod -aG hwlens "$SUDO_USER"
		echo "hwlens: added $SUDO_USER to the hwlens group; log in again (or run 'newgrp hwlens') to use hwlens"
	fi
	if [ -d /run/systemd/system ]; then
		systemctl daemon-reload
		systemctl enable --now hwlensd.socket
		# On upgrade the running daemon is the old binary
		systemctl try-restart hwlensd.service
	fi
fi
POSTINST

cat > "$STAGE/DEBIAN/prerm" <<'PRERM'
#!/bin/sh
set -e
if [ "$1" = remove ] && [ -d /run/systemd/system ]; then
	systemctl disable --now hwlensd.socket hwlensd.service || true
fi
PRERM

cat > "$STAGE/DEBIAN/postrm" <<'POSTRM'
#!/bin/sh
set -e
# The minute history (StateDirectory=hwlensd) goes only on purge
if [ "$1" = purge ]; then
	rm -rf /var/lib/hwlensd
fi
if [ -d /run/systemd/system ]; then
	systemctl daemon-reload || true
fi
POSTRM
chmod 0755 "$STAGE/DEBIAN/postinst" "$STAGE/DEBIAN/prerm" "$STAGE/DEBIAN/postrm"

mkdir -p "$DIST"
dpkg-deb --build --root-owner-group "$STAGE" "$DIST/hwlens_${version}_${arch}.deb"
echo "$DIST/hwlens_${version}_${arch}.deb"
