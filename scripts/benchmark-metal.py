#!/usr/bin/env python3
"""Alternate software and Metal on the same benchmark executable and settings."""
import argparse
import json
import pathlib
import re
import statistics
import subprocess
import time


def window_cpu(pid):
    value = subprocess.check_output(["ps", "-p", pid, "-o", "time="], text=True).strip()
    total = 0.0
    for field in re.split(r"[-:]", value):
        total = total * 60 + float(field)
    return total


def median(rows, key):
    return round(statistics.median(row[key] for row in rows), 2)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("executable", type=pathlib.Path)
    parser.add_argument("settings", type=pathlib.Path)
    parser.add_argument("output", type=pathlib.Path)
    parser.add_argument("--trials", type=int, default=2)
    parser.add_argument("--seconds", type=int, default=8, help="Measurement window after startup")
    probes = parser.add_mutually_exclusive_group()
    probes.add_argument("--presentation-probe", action="store_true",
                        help="Compare window presentation, no counter, and offscreen Metal")
    probes.add_argument("--surface-probe", action="store_true",
                        help="Compare CAMetalLayer and IOSurface CALayer")
    parser.add_argument("--breakdown", action="store_true", help="Measure main-thread CPU for geometry and Metal submission")
    parser.add_argument("--cases", nargs="+", choices=["breathing", "gaze", "interaction", "interaction-plain", "idle"],
                        default=["breathing", "gaze", "interaction", "idle"])
    args = parser.parse_args()
    if args.trials < 1:
        parser.error("--trials must be positive")
    if not 8 <= args.seconds <= 300:
        parser.error("--seconds must be between 8 and 300")
    app = args.executable.resolve()
    output = args.output.resolve()
    output.mkdir(parents=True)
    settings = json.loads(args.settings.read_text())
    settings["autostart"] = False
    settings["position"] = None
    pid = subprocess.check_output(["pgrep", "-x", "WindowServer"], text=True).split()[0]
    results = []
    variants = {"software": [], "metal": ["--metal-test"]}
    if args.presentation_probe:
        variants = {"metal": ["--metal-test", "--metal-drawable"],
                    "no-counter": ["--metal-test", "--metal-no-present-counter", "--metal-drawable"],
                    "offscreen": ["--metal-test", "--metal-offscreen"]}
    if args.surface_probe:
        variants = {"metal": ["--metal-test", "--metal-drawable"],
                    "surface": ["--metal-test", "--metal-surface"],
                    "surface-pool": ["--metal-test", "--metal-surface", "--metal-pool-surfaces"]}
    for name in args.cases:
        settings_for_run = settings.copy()
        if name == "idle":
            settings_for_run["idleAnimation"] = False
        if name == "interaction-plain":
            settings_for_run.update(typingLetters=False, patEnabled=False, teaseEnabled=False, musicNotes=False)
        scenario = {"breathing": "steady", "idle": "steady", "interaction-plain": "interaction"}.get(name, name)
        for trial in range(args.trials):
            backends = list(variants)
            if trial % 2:
                backends.reverse()
            for backend in backends:
                directory = output / f"{name}-{backend}-{trial}"
                directory.mkdir()
                (directory / "settings.json").write_text(json.dumps(settings_for_run), encoding="utf-8")
                command = [str(app), "--cpu-test", "--cpu-scenario", scenario, "--data-dir", str(directory),
                           "--cpu-seconds", str(args.seconds), *variants[backend]]
                if backend != "software" and args.breakdown:
                    command.append("--metal-breakdown")
                if trial == 0 and name == "gaze" and not args.presentation_probe:
                    command.append("--metal-snapshot")
                with (directory / "run.log").open("w") as log, subprocess.Popen(command, stdout=log, stderr=log) as proc:
                    time.sleep(4)
                    start_cpu = window_cpu(pid)
                    start = time.monotonic()
                    time.sleep(args.seconds - 2)
                    elapsed = time.monotonic() - start
                    delta = window_cpu(pid) - start_cpu
                    if proc.wait(timeout=20) != 0:
                        raise RuntimeError(f"Benchmark failed: {directory / 'run.log'}")
                row = json.loads((directory / "cpu-report.json").read_text())
                row.update(case=name, trial=trial, backend=backend, windowServerCPU=delta * 100 / elapsed,
                           windowSampleSeconds=elapsed)
                row["combinedCPU"] = row["cpuPercent"] + row["windowServerCPU"]
                results.append(row)
                (output / "results.json").write_text(json.dumps(results, indent=2), encoding="utf-8")
                print(name, trial, backend, f"app={row['cpuPercent']:.2f}%",
                      f"WindowServer={row['windowServerCPU']:.2f}%", f"fps={row['renderFPS']:.2f}",
                      f"metal={row['metalFrames']} software={row['softwareFrames']} presented={row['metalPresented']}", flush=True)
        for backend in variants:
            rows = [row for row in results if row["case"] == name and row["backend"] == backend]
            keys = ["cpuPercent", "windowServerCPU", "combinedCPU", "displayHz", "renderFPS", "metalPresented", "physicalFootprint", "peakFootprint"]
            if args.surface_probe:
                keys.append("surfaceLayerUpdates")
            if args.breakdown and backend != "software":
                keys.extend(("mainThreadProjectCPU", "mainThreadSubmitCPU"))
            print("median", name, backend, {key: median(rows, key) for key in keys}, flush=True)


if __name__ == "__main__":
    main()
