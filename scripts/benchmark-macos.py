#!/usr/bin/env python3
"""Run an isolated CPU matrix against an app built with --cpu-test support.
Does not change the installed app, real settings, permissions or music players.
Usage: python3 scripts/benchmark-macos.py /path/Minecraft桌宠.app /path/results --trials 3
"""
import argparse
import json
import pathlib
import platform
import plistlib
import statistics
import struct
import subprocess
import zlib

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("app", type=pathlib.Path)
parser.add_argument("output", type=pathlib.Path)
parser.add_argument("--trials", type=int, default=3)
parser.add_argument("--cases", nargs="+", help="Only run the named cases")
parser.add_argument("--baseline", type=pathlib.Path, help="Alternate matched trials with an instrumented baseline app")
args = parser.parse_args()
if args.trials < 1:
    parser.error("--trials must be positive")
apps = [("optimized", args.app.resolve() / "Contents/MacOS/MinecraftPet")]
if args.baseline:
    apps.insert(0, ("baseline", args.baseline.resolve() / "Contents/MacOS/MinecraftPet"))
output = args.output.resolve()
# Refuse to overwrite prior results/configuration.
output.mkdir(parents=True, exist_ok=False)
metadata = {"machine": platform.machine(), "macOS": platform.mac_ver()[0], "trials": args.trials,
            "warmupSeconds": 3, "measurementSeconds": 8, "apps": {}}
for variant, executable in apps:
    with (executable.parent.parent / "Info.plist").open("rb") as f:
        info = plistlib.load(f)
    metadata["apps"][variant] = {"path": str(executable), "version": info.get("CFBundleShortVersionString")}
(output / "metadata.json").write_text(json.dumps(metadata, indent=2), encoding="utf-8")
pixels = bytearray([215, 163, 121, 255] * 64 * 64)
for x, color in [(4, [242, 228, 201, 255]), (5, [59, 97, 76, 255])]:
    pixels[(7 * 64 + x) * 4:(7 * 64 + x + 1) * 4] = bytes(color)

def chunk(kind, data):
    return (struct.pack(">I", len(data)) + kind + data
            + struct.pack(">I", zlib.crc32(kind + data) & 0xffffffff))

skin = output / "blinkable.png"
scanlines = b"".join(b"\0" + pixels[y * 256:(y + 1) * 256] for y in range(64))
skin.write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">2I5B", 64, 64, 8, 6, 0, 0, 0))
                 + chunk(b"IDAT", zlib.compress(scanlines)) + chunk(b"IEND", b""))
cases = [(f"breathing-{scale}-{fps}", dict(scale=scale, fps=fps, idleAnimation=True), "steady")
         for scale in (0.65, 1.0) for fps in (30, 60, 120)]
cases += [("idle", {}, "steady"), ("blink", dict(daBlink=True), "steady"),
          ("interaction", dict(patEnabled=True, keyboardEnabled=True), "interaction"),
          ("gaze", {}, "gaze"),
          ("hidden", dict(idleAnimation=True), "hidden"), ("console", {}, "console")]
if args.cases:
    unknown = set(args.cases) - {name for name, _, _ in cases}
    if unknown:
        parser.error("Unknown cases: " + ", ".join(sorted(unknown)))
    cases = [case for case in cases if case[0] in args.cases]
results = []
for name, overrides, scenario in cases:
    for trial in range(args.trials):
        # Reverse order every other trial to reduce warm-up/thermal order bias.
        for variant, app in (apps if trial % 2 == 0 else list(reversed(apps))):
            directory = output / f"{name}-{variant}-{trial}"
            directory.mkdir()
            config = dict(posture="sitting", chibi=True, trackMouse=False, keyboardEnabled=False,
                          hypQuickChat=False, autostart=False, daBlink=False, daDance=False,
                          idleAnimation=False, musicNotes=False, occasionalStretch=False,
                          occasionalTilt=False, occasionalSleepy=False, fps=120, scale=0.65,
                          keyboardStyle="simple", skinPath=str(skin))
            config.update(overrides)
            (directory / "settings.json").write_text(json.dumps(config), encoding="utf-8")
            with (directory / "run.log").open("w") as log:
                subprocess.run([str(app), "--cpu-test", "--cpu-scenario", scenario,
                                "--data-dir", str(directory)], stdout=log, stderr=log,
                               check=True, timeout=40)
            result = json.loads((directory / "cpu-report.json").read_text())
            result.update(case=name, trial=trial, variant=variant)
            results.append(result)
    for variant, _ in apps:
        rows = [r for r in results if r["case"] == name and r["variant"] == variant]
        print(name, variant, "CPU%", round(statistics.median(r["cpuPercent"] for r in rows), 2),
              "render FPS", round(statistics.median(r["renderFPS"] for r in rows), 2), flush=True)
    (output / "results.json").write_text(json.dumps(results, indent=2), encoding="utf-8")
