#!/usr/bin/env python3
"""生成用于交叉编译 Linux 前端的 Debian bookworm sysroot。

用法：debian-sysroot.py ARCH OUTDIR
      ARCH = amd64 | arm64 | i386

两步：
1. 解析 libgtk-3-dev 的 Depends/Pre-Depends 闭包（只跟随库与开发包），下载解包；
2. 反复用 pkg-config 检查 gtk+-3.0，把仍缺的 .pc 所属包补进来
   （.pc 文件名通过 Debian Contents 索引反查，比 Provides 可靠）。

用 bookworm（glibc 2.36）而不是宿主机的滚动发行版，产出的二进制才能跑在
Debian 12 / Ubuntu 22.04+ 上。需要能访问 Debian 镜像与 pkg-config、ar、zstd。
"""
import gzip
import io
import lzma
import os
import re
import subprocess
import sys
import tarfile
import urllib.request
from concurrent.futures import ThreadPoolExecutor

MIRROR = os.environ.get("PET_DEBIAN_MIRROR", "https://mirrors.tuna.tsinghua.edu.cn/debian")
SUITE = os.environ.get("PET_DEBIAN_SUITE", "bookworm")
ROOTS = ["libgtk-3-dev", "libc6-dev", "libgcc-12-dev", "linux-libc-dev"]
# 这些只需要头文件/链接库，其余工具与数据包跳过（python/perl/字体/图标缓存等）
SKIP = re.compile(
    r"^(python3.*|perl.*|debconf.*|dpkg.*|bash|coreutils|sensible-utils|"
    r"pkg-config|pkgconf.*|gir1\.2-.*|libgirepository.*|gobject-introspection.*|"
    r"dconf-.*|adwaita-icon-theme|hicolor-icon-theme|shared-mime-info|"
    r"gtk-update-icon-cache|libgtk-3-common|libgtk-3-bin|fontconfig-config|"
    r"fonts-.*|ucf|tzdata|install-info|xkb-data|x11-common|libc-dev-bin|"
    r"libglib2\.0-bin|libglib2\.0-dev-bin|libglib2\.0-data|libgdk-pixbuf2\.0-bin|"
    r"libgdk-pixbuf2\.0-common|libpango1\.0-doc|.*-doc|.*-data|gcc-12|cpp-12|"
    r"binutils.*|libcrypt-dev|rpcsvc-proto|libnsl-dev|libtirpc-dev|"
    r"libsepol-dev|libselinux1-dev|libpcre2-dev|libmount-dev|libblkid-dev|"
    r"libffi-dev|zlib1g-dev|uuid-dev|libdbus-1-dev|libsystemd-dev|libcolord.*|"
    r"libcups2.*|libavahi.*|libgnutls.*|libkrb5.*|libgssapi.*)$"
)
# 这些 -dev 的 .pc 与 .so 符号链接是链接 gtk 必需的，不能跳过
KEEP_DEV = {
    "zlib1g-dev", "libffi-dev", "libpcre2-dev", "libmount-dev", "libblkid-dev",
    "libselinux1-dev", "libsepol-dev", "uuid-dev", "libdbus-1-dev",
}


def fetch(url):
    err = None
    for _ in range(3):
        try:
            with urllib.request.urlopen(url, timeout=90) as r:
                return r.read()
        except Exception as e:
            err = e
    raise SystemExit(f"下载失败 {url}: {err}")


def parse_packages(text):
    pkgs = {}
    for block in text.split("\n\n"):
        fields, key = {}, None
        for line in block.splitlines():
            if line[:1] in (" ", "\t") and key:
                fields[key] += "\n" + line
            elif ":" in line:
                key, _, val = line.partition(":")
                fields[key] = val.strip()
        name = fields.get("Package")
        if name:
            pkgs.setdefault(name, fields)
    return pkgs


def deps(fields):
    for key in ("Pre-Depends", "Depends"):
        for group in fields.get(key, "").split(","):
            group = group.strip()
            if group:
                yield [a.strip().split(" ")[0].split(":")[0] for a in group.split("|")]


def fix_links(root):
    """绝对路径的符号链接改写成 sysroot 内的相对路径。"""
    for dirpath, dirnames, filenames in os.walk(root):
        for name in dirnames + filenames:
            p = os.path.join(dirpath, name)
            if os.path.islink(p) and os.readlink(p).startswith("/"):
                target = os.readlink(p)
                os.unlink(p)
                os.symlink(os.path.relpath(root + target, dirpath), p)


def main():
    arch, root = sys.argv[1], os.path.abspath(sys.argv[2])
    multi = {"amd64": "x86_64-linux-gnu", "arm64": "aarch64-linux-gnu",
             "i386": "i386-linux-gnu"}[arch]
    os.makedirs(root, exist_ok=True)
    cache = os.path.join(os.path.dirname(root), f"debs-{arch}")
    os.makedirs(cache, exist_ok=True)

    pkgs = parse_packages(
        lzma.decompress(fetch(f"{MIRROR}/dists/{SUITE}/main/binary-{arch}/Packages.xz")).decode()
    )
    contents = {}
    for line in gzip.decompress(
        fetch(f"{MIRROR}/dists/{SUITE}/main/Contents-{arch}.gz")
    ).decode("utf-8", "replace").splitlines():
        parts = line.rsplit(None, 1)
        if len(parts) != 2 or "debug-build" in parts[0]:
            continue
        base = parts[0].rsplit("/", 1)[-1]
        if base.endswith(".pc"):
            contents[base[:-3]] = parts[1].split(",")[0].rsplit("/", 1)[-1]

    def install(name):
        fields = pkgs[name]
        path = os.path.join(cache, os.path.basename(fields["Filename"]))
        if not os.path.exists(path):
            with open(path + ".part", "wb") as f:
                f.write(fetch(f"{MIRROR}/{fields['Filename']}"))
            os.rename(path + ".part", path)
        data = next(m for m in subprocess.run(["ar", "t", path], capture_output=True,
                                              text=True).stdout.split()
                    if m.startswith("data.tar"))
        blob = subprocess.run(["ar", "p", path, data], capture_output=True).stdout
        if data.endswith(".zst"):
            blob = subprocess.run(["zstd", "-dc"], input=blob, capture_output=True).stdout
            mode = "r:"
        else:
            mode = "r:*"
        with tarfile.open(fileobj=io.BytesIO(blob), mode=mode) as tar:
            tar.extractall(root, filter="tar")
        fix_links(root)

    # 第一步：Depends 闭包
    want, todo = set(), list(ROOTS)
    while todo:
        name = todo.pop()
        if name not in pkgs or name in want:
            continue
        if SKIP.match(name) and name not in KEEP_DEV:
            continue
        want.add(name)
        for alts in deps(pkgs[name]):
            for alt in alts:
                if alt in pkgs:
                    todo.append(alt)
                    break
    with ThreadPoolExecutor(8) as pool:
        list(pool.map(install, sorted(want)))
    print(f"{arch}: 依赖闭包 {len(want)} 个包")

    # 第二步：补 .pc
    env = dict(os.environ,
               PKG_CONFIG_LIBDIR=f"{root}/usr/lib/{multi}/pkgconfig:{root}/usr/lib/pkgconfig:{root}/usr/share/pkgconfig",
               PKG_CONFIG_SYSROOT_DIR=root)
    for _ in range(40):
        r = subprocess.run(["pkg-config", "--cflags", "--libs", "gtk+-3.0"],
                           capture_output=True, text=True, env=env)
        if r.returncode == 0:
            print(f"{arch}: gtk+-3.0 依赖已闭合 → {root}")
            return
        missing = set(re.findall(r"Package '([^']+)', required by", r.stderr))
        found = {m: contents[m] for m in missing if m in contents and contents[m] in pkgs}
        if not found:
            raise SystemExit(f"{arch}: 无法继续，剩余报错：\n{r.stderr[:1500]}")
        for m, name in sorted(found.items()):
            print(f"{arch}: 补 {m} -> {name}")
            install(name)


if __name__ == "__main__":
    main()
