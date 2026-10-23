# Third-party notices — Minecraft桌宠

Original application code and example skin: MIT. Independent fan project, not affiliated with Mojang or Microsoft.

The Windows executable is built from Rust source. It does not bundle Electron, Chromium, Node.js, WebView2, or Three.js.

Dependency licenses and copyright notices are included under native/licenses/ (licenses/ in the release ZIP). The lockfile includes build dependencies and Windows target support packages.

| Package | Version | License |
| --- | --- | --- |
| adler2 | 2.0.1 | 0BSD OR MIT OR Apache-2.0 |
| autocfg | 1.5.1 | Apache-2.0 OR MIT |
| bitflags | 2.13.2 | MIT OR Apache-2.0 |
| bytemuck | 1.25.2 | Zlib OR Apache-2.0 OR MIT |
| byteorder-lite | 0.1.0 | Unlicense OR MIT |
| cfg-if | 1.0.5 | MIT OR Apache-2.0 |
| crc32fast | 1.5.2 | MIT OR Apache-2.0 |
| fdeflate | 0.3.7 | MIT OR Apache-2.0 |
| flate2 | 1.1.10 | MIT OR Apache-2.0 |
| glam | 0.30.10 | MIT OR Apache-2.0 |
| image | 0.25.10 | MIT OR Apache-2.0 |
| itoa | 1.0.18 | MIT OR Apache-2.0 |
| memchr | 2.8.3 | Unlicense OR MIT |
| miniz_oxide | 0.8.9 | MIT OR Zlib OR Apache-2.0 |
| miniz_oxide | 0.9.1 | MIT OR Zlib OR Apache-2.0 |
| moxcms | 0.8.1 | BSD-3-Clause OR Apache-2.0 |
| num-traits | 0.2.19 | MIT OR Apache-2.0 |
| png | 0.18.1 | MIT OR Apache-2.0 |
| proc-macro2 | 1.0.107 | MIT OR Apache-2.0 |
| pxfm | 0.1.30 | BSD-3-Clause OR Apache-2.0 |
| quote | 1.0.47 | MIT OR Apache-2.0 |
| serde | 1.0.229 | MIT OR Apache-2.0 |
| serde_core | 1.0.229 | MIT OR Apache-2.0 |
| serde_derive | 1.0.229 | MIT OR Apache-2.0 |
| serde_json | 1.0.154 | MIT OR Apache-2.0 |
| simd-adler32 | 0.3.10 | MIT |
| syn | 3.0.8 | MIT OR Apache-2.0 |
| unicode-ident | 1.0.27 | (MIT OR Apache-2.0) AND Unicode-3.0 |
| windows-sys | 0.59.0 | MIT OR Apache-2.0 |
| windows-targets | 0.52.6 | MIT OR Apache-2.0 |
| windows_x86_64_gnu | 0.52.6 | MIT OR Apache-2.0 |
| zlib-rs | 0.6.8 | Zlib |
| zmij | 1.0.23 | MIT |

Rust standard library notices and toolchain runtime notices are in the toolchain subdirectory.

GitHub Octicons mark-github icon: Copyright GitHub, Inc., MIT license. Source: https://github.com/primer/octicons/blob/main/icons/mark-github-16.svg . License included under native/licenses/github-octicons/ (licenses/github-octicons/ in the release ZIP). GitHub marks belong to GitHub; their use links to the project repository and does not imply endorsement.


## Animation and texture provenance (v2.5)

# 资源来源

喂食动作参照用户指定的 https://dlmember.github.io/Minecraft-SkinPop/ ，源文件 src/eating.js；站立32tick动作参数和苹果碎屑按该实现移植，Q版增加嘴部前方适配及回位过渡。
苹果 PNG 取自该仓库 src/assets/apple.png，出处为 Minecraft Java 1.21.4。对应源说明存于 native/assets/apple-provenance.json。
音符 PNG 来源：https://github.com/InventivetalentDev/minecraft-assets/blob/1.21.4/assets/minecraft/textures/particle/note.png
GitHub 图标复用本项目既有 assets/github.ico。
Minecraft 资源属于各自权利人。本项目与 Mojang / Microsoft 无隶属、赞助或官方合作关系。


The Q-style model and original eye/Shift animation support are based on zsyghxf's PR #5 (commit 7ba3b87862272b13f7f10cdb4b225ced1b9350d6), with behavior and interaction changes by DLmember. DA animation adjustments reference the user-provided DetailedAnimationsXJustExpressions resource pack; the pack itself and user skins/capes are not bundled. Microsoft YaHei UI is accessed through the installed Windows font; no font file is redistributed.
