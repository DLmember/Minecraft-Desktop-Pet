#!/bin/bash
# 从交叉编译产物生成 deb 与 Arch 包（x86_64 / aarch64 / i686）。
# 二进制须由 scripts/build-linux-cross.sh 用 Debian bookworm sysroot 编出，
# 否则会带上宿主机的新 glibc 符号，旧发行版无法运行。
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/linux/Cargo.toml" | head -1)"
DEST="${1:-$ROOT/release/packages}"
WORK="${WORK:-/tmp/opencode/pkgwork}"
NAME=minecraft-desktop-pet
mkdir -p "$DEST"

# arch 三元组: rust-target deb-arch arch-carch
ARCHES=(
    "x86_64-unknown-linux-gnu amd64 x86_64"
    "aarch64-unknown-linux-gnu arm64 aarch64"
    "i686-unknown-linux-gnu i386 i686"
)

prepare_docs() { # $1 = 目标目录
    local d=$1
    install -Dm644 "$ROOT/linux/packaging/minecraft-desktop-pet.png" \
        "$d/usr/share/icons/hicolor/256x256/apps/$NAME.png"
    install -Dm644 "$ROOT/linux/packaging/minecraft-desktop-pet.desktop" \
        "$d/usr/share/applications/$NAME.desktop"
    install -Dm644 "$ROOT/LICENSE" "$d/usr/share/doc/$NAME/copyright"
    install -Dm644 "$ROOT/THIRD_PARTY_NOTICES.md" "$d/usr/share/doc/$NAME/THIRD_PARTY_NOTICES.md"
    install -Dm644 "$ROOT/linux/README.md" "$d/usr/share/doc/$NAME/README-Linux.md"
    cp -R "$ROOT/native/licenses" "$d/usr/share/doc/$NAME/licenses"
    chmod -R u+rwX,go+rX,go-w "$d/usr/share/doc/$NAME/licenses"
}

build_deb() { # $1 rust-target $2 deb-arch
    local target=$1 arch=$2 bin="$ROOT/linux/target/$1/release/$NAME"
    [ -x "$bin" ] || { echo "缺少 $bin，先跑 scripts/build-linux-cross.sh" >&2; return 1; }
    local stage="$WORK/deb-$arch"
    rm -rf "$stage"; mkdir -p "$stage/DEBIAN"
    install -Dm755 "$bin" "$stage/usr/bin/$NAME"
    prepare_docs "$stage"
    # 桌面文件里的 Exec 用 PATH 中的名字，装到 /usr/bin 后无需改写
    cat > "$stage/DEBIAN/control" <<EOF
Package: $NAME
Version: $VERSION
Architecture: $arch
Maintainer: Minecraft-Desktop-Pet contributors
Installed-Size: $(du -sk --exclude=DEBIAN "$stage" | cut -f1)
Depends: libgtk-3-0 (>= 3.24) | libgtk-3-0t64 (>= 3.24), libglib2.0-0 | libglib2.0-0t64, libgcc-s1, libc6
Recommends: curl, xdg-utils
Section: utils
Priority: optional
Homepage: https://github.com/
Description: Minecraft 风格的桌面宠物
 透明置顶的 Minecraft 桌面宠物：复用原生 Rust 渲染器，支持皮肤导入、
 正版皮肤与披风获取、全局键鼠互动、鼠标穿透、系统托盘与开机自启。
EOF
    cat > "$stage/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e
if [ "$1" = configure ]; then
    command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database -q /usr/share/applications || true
    command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -t /usr/share/icons/hicolor || true
fi
exit 0
EOF
    cp "$stage/DEBIAN/postinst" "$stage/DEBIAN/postrm"
    chmod 755 "$stage/DEBIAN/postinst" "$stage/DEBIAN/postrm"
    ( cd "$stage" && find usr -type f -exec md5sum {} + > DEBIAN/md5sums && chmod 644 DEBIAN/md5sums )
    chmod 755 "$stage/DEBIAN"
    chmod 644 "$stage/DEBIAN/control"
    # debian-binary / control.tar.xz / data.tar.xz 必须放在 stage 外，
    # 否则 data.tar.xz 会把自己包进去
    local tars="$WORK/tars-$arch"
    rm -rf "$tars"; mkdir -p "$tars"
    printf '2.0\n' > "$tars/debian-binary"
    tar -C "$stage/DEBIAN" -cJf "$tars/control.tar.xz" .
    tar -C "$stage" --exclude=DEBIAN -cJf "$tars/data.tar.xz" .
    local out="$DEST/${NAME}_${VERSION}_${arch}.deb"
    rm -f "$out"
    ( cd "$tars" && bsdtar -cf "$out" --format=ar debian-binary control.tar.xz data.tar.xz )
    echo "deb: $out"
}

build_arch() { # $1 rust-target $2 pkg-arch $3 carch
    local target=$1 arch=$2 carch=$3 bin="$ROOT/linux/target/$1/release/$NAME"
    [ -x "$bin" ] || { echo "缺少 $bin，先跑 scripts/build-linux-cross.sh" >&2; return 1; }
    local stage="$WORK/arch-$arch"
    rm -rf "$stage"; mkdir -p "$stage"
    install -m755 "$bin" "$stage/$NAME"
    cat > "$stage/PKGBUILD" <<EOF
pkgname=$NAME
pkgver=$VERSION
pkgrel=1
pkgdesc="Minecraft 风格的桌面宠物"
arch=('$arch')
url="https://github.com/"
license=('MIT')
depends=('gtk3' 'glib2' 'glibc' 'gcc-libs')
optdepends=('curl: 获取正版皮肤与披风' 'xdg-utils: 打开链接')
options=('!strip' '!debug')
source=()

package() {
    install -Dm755 "\$startdir/$NAME" "\$pkgdir/usr/bin/$NAME"
    install -Dm644 "\$startdir/desktop" "\$pkgdir/usr/share/applications/$NAME.desktop"
    install -Dm644 "\$startdir/icon.png" "\$pkgdir/usr/share/icons/hicolor/256x256/apps/$NAME.png"
    install -Dm644 "\$startdir/copyright" "\$pkgdir/usr/share/licenses/$NAME/LICENSE"
    install -Dm644 "\$startdir/THIRD_PARTY_NOTICES.md" "\$pkgdir/usr/share/licenses/$NAME/THIRD_PARTY_NOTICES.md"
    install -Dm644 "\$startdir/README-Linux.md" "\$pkgdir/usr/share/doc/$NAME/README-Linux.md"
    cp -R "\$startdir/licenses" "\$pkgdir/usr/share/licenses/$NAME/licenses"
}
EOF
    cp "$ROOT/linux/packaging/minecraft-desktop-pet.desktop" "$stage/desktop"
    cp "$ROOT/linux/packaging/minecraft-desktop-pet.png" "$stage/icon.png"
    cp "$ROOT/LICENSE" "$stage/copyright"
    cp "$ROOT/THIRD_PARTY_NOTICES.md" "$stage/THIRD_PARTY_NOTICES.md"
    cp "$ROOT/linux/README.md" "$stage/README-Linux.md"
    cp -R "$ROOT/native/licenses" "$stage/licenses"
    # 交叉打包：覆盖 CARCH/CHOST，避免 makepkg 按宿主机架构拒绝
    local conf="$stage/makepkg.conf"
    sed -e "s|^CARCH=.*|CARCH=\"$carch\"|" \
        -e "s|^CHOST=.*|CHOST=\"$carch-pc-linux-gnu\"|" \
        /etc/makepkg.conf > "$conf"
    ( cd "$stage" && PKGDEST="$DEST" makepkg -f --nodeps --skipinteg --config "$conf" ) | tail -5
    ls "$DEST"/$NAME-$VERSION-*.pkg.tar.* 2>/dev/null | tail -1 | sed 's/^/arch: /'
}

# 可选：用 PET_ARCHES 只打指定架构（如 PET_ARCHES="amd64"）
if [ -n "${PET_ARCHES:-}" ]; then
    FILTERED=()
    for entry in "${ARCHES[@]}"; do
        read -r _ debarch _ <<< "$entry"
        [[ " $PET_ARCHES " == *" $debarch "* ]] && FILTERED+=("$entry")
    done
    ARCHES=("${FILTERED[@]}")
fi

for entry in "${ARCHES[@]}"; do
    read -r target debarch carch <<< "$entry"
    echo "== $debarch"
    build_deb "$target" "$debarch"
    build_arch "$target" "$carch" "$carch"
done
echo "输出目录：$DEST"
