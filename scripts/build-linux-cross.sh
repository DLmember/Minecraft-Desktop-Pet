#!/bin/bash
# 交叉编译 Linux 前端（amd64 / arm64 / i386）。
# 需要：rustup target add aarch64-unknown-linux-gnu i686-unknown-linux-gnu，
#       scripts/debian-sysroot.py 生成的 sysroot，以及 clang + lld。
#
# 用法：scripts/build-linux-cross.sh [SYSROOT_ROOT]
#   SYSROOT_ROOT 下需有 sysroot-amd64 / sysroot-arm64 / sysroot-i386
#   默认 /tmp/opencode/xbuild
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
X="${1:-${PET_SYSROOT_ROOT:-/tmp/opencode/xbuild}}"
WORK="${WORK:-$X/wrappers}"
mkdir -p "$WORK"

build() { # deb-arch rust-target clang-triple multiarch
    local deb=$1 rust=$2 triple=$3 multi=$4
    local sys="$X/sysroot-$deb"
    [ -d "$sys/usr/include/gtk-3.0" ] || {
        echo "缺少 sysroot：$sys（先运行 scripts/debian-sysroot.py $deb $sys）" >&2
        return 1
    }
    local cc="$WORK/cc-$deb"
    cat > "$cc" <<EOF
#!/bin/sh
exec clang --target=$triple --sysroot=$sys -fuse-ld=lld "\$@"
EOF
    chmod +x "$cc"
    local upper
    upper="$(echo "$rust" | tr 'a-z-' 'A-Z_')"
    echo "== $deb ($rust)"
    env "CARGO_TARGET_${upper}_LINKER=$cc" \
        "CC_${rust//-/_}=$cc" \
        PKG_CONFIG_ALLOW_CROSS=1 \
        PKG_CONFIG_SYSROOT_DIR="$sys" \
        PKG_CONFIG_LIBDIR="$sys/usr/lib/$multi/pkgconfig:$sys/usr/lib/pkgconfig:$sys/usr/share/pkgconfig" \
        cargo build --manifest-path "$ROOT/linux/Cargo.toml" --locked --release --target "$rust"
}

build amd64 x86_64-unknown-linux-gnu x86_64-linux-gnu x86_64-linux-gnu
build arm64 aarch64-unknown-linux-gnu aarch64-linux-gnu aarch64-linux-gnu
build i386 i686-unknown-linux-gnu i686-linux-gnu i386-linux-gnu
echo "产物：$ROOT/linux/target/<target>/release/minecraft-desktop-pet"
