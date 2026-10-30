#!/bin/bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="${1:-$ROOT/release/linux}"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/linux/Cargo.toml" | head -1)"

cargo build --manifest-path "$ROOT/linux/Cargo.toml" --locked --release

PKG="$DEST/package"
rm -rf "$PKG"
mkdir -p "$PKG"
install -m755 "$ROOT/linux/target/release/minecraft-desktop-pet" "$PKG/"
cp "$ROOT/LICENSE" "$ROOT/THIRD_PARTY_NOTICES.md" "$PKG/"
cp "$ROOT/linux/README.md" "$PKG/使用说明-Linux.md"
cp "$ROOT/linux/install.sh" "$PKG/install.sh"
chmod +x "$PKG/install.sh"
cp "$ROOT/linux/packaging/minecraft-desktop-pet.desktop" "$PKG/"
cp "$ROOT/linux/packaging/minecraft-desktop-pet.png" "$PKG/"
cp -R "$ROOT/native/licenses" "$PKG/"

tar -C "$PKG" -czf "$DEST/Minecraft桌宠-${VERSION}-linux-x86_64.tar.gz" .
echo "Built: $DEST/Minecraft桌宠-${VERSION}-linux-x86_64.tar.gz"
echo "Binary: $ROOT/linux/target/release/minecraft-desktop-pet"
