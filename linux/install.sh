#!/bin/bash
set -euo pipefail
SELF_DIR="$(cd "$(dirname "$0")" && pwd)"

BIN="${1:-}"
if [ -z "$BIN" ]; then
    if [ -x "$SELF_DIR/minecraft-desktop-pet" ]; then
        BIN="$SELF_DIR/minecraft-desktop-pet"
    elif [ -x "$SELF_DIR/linux/target/release/minecraft-desktop-pet" ]; then
        BIN="$SELF_DIR/linux/target/release/minecraft-desktop-pet"
    elif [ -x "$SELF_DIR/target/release/minecraft-desktop-pet" ]; then
        BIN="$SELF_DIR/target/release/minecraft-desktop-pet"
    else
        echo "找不到 minecraft-desktop-pet，请先运行 scripts/build-linux.sh" >&2
        exit 1
    fi
fi

if [ -f "$SELF_DIR/minecraft-desktop-pet.png" ]; then
    ICON="$SELF_DIR/minecraft-desktop-pet.png"
    DESKTOP="$SELF_DIR/minecraft-desktop-pet.desktop"
else
    ICON="$SELF_DIR/packaging/minecraft-desktop-pet.png"
    DESKTOP="$SELF_DIR/packaging/minecraft-desktop-pet.desktop"
fi

PREFIX="${PREFIX:-$HOME/.local}"
BINDIR="$PREFIX/bin"
APPDIR="$PREFIX/share/applications"
ICONDIR="$PREFIX/share/icons/hicolor/256x256/apps"

install -Dm755 "$BIN" "$BINDIR/minecraft-desktop-pet"
install -Dm644 "$ICON" "$ICONDIR/minecraft-desktop-pet.png"
install -Dm644 "$DESKTOP" "$APPDIR/minecraft-desktop-pet.desktop"
sed -i "s|^Exec=.*|Exec=$BINDIR/minecraft-desktop-pet|" "$APPDIR/minecraft-desktop-pet.desktop"

if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$APPDIR" >/dev/null 2>&1 || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -q -t "$PREFIX/share/icons/hicolor" >/dev/null 2>&1 || true
fi

echo "已安装：$BINDIR/minecraft-desktop-pet"
echo "桌面项：$APPDIR/minecraft-desktop-pet.desktop"
