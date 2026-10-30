#!/bin/bash
# 把交叉编译的 Windows 产物打成便携版：单文件 exe + 含说明与许可的 zip。
# 需要先用 mingw-w64（x64/x86）与 MSYS2 clangarm64 sysroot（arm64）构建。
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="${1:-$ROOT/release/windows}"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/native/Cargo.toml" | head -1)"
EXE_NAME="Minecraft桌宠.exe"
mkdir -p "$DEST"

# 后缀 rust-target 标签
ENTRIES=(
    "x86_64-pc-windows-gnu x64"
    "i686-pc-windows-gnu x86"
    "aarch64-pc-windows-gnullvm arm64"
)

: > "$DEST/SHA256SUMS-Windows.txt"
for entry in "${ENTRIES[@]}"; do
    read -r target label <<< "$entry"
    bin="$ROOT/native/target/$target/release/minecraft-pet.exe"
    [ -f "$bin" ] || { echo "缺少 $bin" >&2; exit 1; }
    stage="$DEST/stage-$label"
    rm -rf "$stage"; mkdir -p "$stage"
    install -m644 "$bin" "$stage/$EXE_NAME"
    docs=("$ROOT/LICENSE" "$ROOT/THIRD_PARTY_NOTICES.md")
    # 使用说明可能不在源码树里（例如被移出仓库），有才带上
    [ -f "$ROOT/使用说明.md" ] && docs+=("$ROOT/使用说明.md")
    install -m644 "${docs[@]}" "$stage/"
    cp -R "$ROOT/native/licenses" "$stage/licenses"
    ( cd "$stage" && sha256sum "$EXE_NAME" > SHA256SUMS.txt )
    pkg="$DEST/Minecraft-Desktop-Pet-${VERSION}-windows-$label"
    rm -f "$pkg.zip"
    bsdtar -a -cf "$pkg.zip" -C "$stage" .
    cp "$stage/$EXE_NAME" "$pkg.exe"
    ( cd "$DEST" && sha256sum "Minecraft-Desktop-Pet-${VERSION}-windows-$label.exe" \
        "Minecraft-Desktop-Pet-${VERSION}-windows-$label.zip" >> SHA256SUMS-Windows.txt )
    rm -rf "$stage"
    echo "windows-$label: $pkg.zip"
done
echo "SHA256SUMS: $DEST/SHA256SUMS-Windows.txt"
