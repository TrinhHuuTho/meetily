#!/usr/bin/env bash
# Tauri names the Debian package after productName in kebab case ("TriNote" ->
# "tri-note"). Rename it to "trinote", the name used by `apt remove trinote` and
# in the docs. Usage: rename-deb-package.sh <package.deb> [name]
set -euo pipefail

deb="$1"
name="${2:-trinote}"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

dpkg-deb -R "$deb" "$work/pkg"
sed -i "s/^Package: .*/Package: $name/" "$work/pkg/DEBIAN/control"
dpkg-deb --root-owner-group -Zxz --build "$work/pkg" "$deb"
echo "$(dpkg-deb -f "$deb" Package) $(dpkg-deb -f "$deb" Version) -> $deb"
