"""M1 sync test driver (plan §8.3 wp-sync-test), started by scripts/sync/run.sh inside the live
lock. Standard library only."""

import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
RUNSH = os.path.join(ROOT, "tools", "macos-dev-host", "run.sh")
CLI = os.environ["SC_SYNC_CLI"]
DEVTOOLS = os.environ["SC_DEVTOOLS"]
KEEP = os.environ.get("SC_SYNC_KEEP") == "1"
RUNS = int(os.environ.get("SC_SYNC_RUNS", "1"))
DURATION_S = 70
SCHEDULE = ("2:start,4:start,38:after-switch-1,40:after-switch-1,"
            "53:after-switch-2,55:after-switch-2,66:end,68:end")
FULLSCREEN = "5:start-camera,65:end-camera"
WORK = tempfile.mkdtemp(prefix="sc-sync-")
OUT = os.path.join(ROOT, "target", "sync", time.strftime("%Y%m%d-%H%M%S"))
LIVE = []


def log(msg):
    print(msg, flush=True)


def read_json_line(stream, want, timeout_s):
    box = {}

    def reader():
        for line in stream:
            try:
                ev = json.loads(line)
            except ValueError:
                continue
            if ev.get("event") == want:
                box["ev"] = ev
                return

    t = threading.Thread(target=reader, daemon=True)
    t.start()
    t.join(timeout_s)
    return box.get("ev")


def one_run(n):
    run_dir = os.path.join(WORK, f"run{n}")
    os.makedirs(run_dir)
    fb_log = os.path.join(run_dir, "flashbeep.json")
    fb = subprocess.Popen(
        [DEVTOOLS, "sync", "flashbeep", "--schedule", SCHEDULE, "--fullscreen-at", FULLSCREEN,
         "--out", fb_log, "--start-on-signal", "--linger", "1"],
        stdout=subprocess.PIPE, text=True, cwd=os.path.dirname(DEVTOOLS))
    LIVE.append(fb)
    ready = read_json_line(fb.stdout, "ready", 300)
    if not ready:
        raise RuntimeError("flashbeep did not report ready")
    wid = ready["window_id"]
    log(f"run {n}: SC-SYNC-WIN window {wid}")

    pid_file = os.path.join(run_dir, "child.pid")
    cmd = [RUNSH, CLI, "record", "--source", "display:main",
           "--switch", f"35=window:{wid}", "--switch", "50=display:main",
           "--mic", "default", "--system-audio", "all", "--camera", "default",
           "--duration", str(DURATION_S), "--events",
           "--projects-dir", os.path.join(run_dir, "projects")]
    rec = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
                           env=dict(os.environ, SC_DEV_PID_FILE=pid_file), cwd=run_dir,
                           start_new_session=True)
    LIVE.append(rec)
    lines, errs = [], []

    def read_out():
        for line in rec.stdout:
            line = line.rstrip("\n")
            lines.append(line)
            if line.startswith("{") and '"started"' in line:
                fb.send_signal(signal.SIGUSR1)
                log(f"run {n}: recording started")

    def read_err():
        for line in rec.stderr:
            errs.append(line.rstrip("\n"))

    threads = [threading.Thread(target=read_out, daemon=True),
               threading.Thread(target=read_err, daemon=True)]
    for t in threads:
        t.start()
    try:
        rec.wait(timeout=DURATION_S + 120)
    except subprocess.TimeoutExpired:
        log(f"run {n}: recording timed out, killing it")
        try:
            os.kill(int(open(pid_file).read().strip()), signal.SIGKILL)
        except (OSError, ValueError):
            pass
        os.killpg(rec.pid, signal.SIGKILL)
        rec.wait()
    for t in threads:
        t.join(timeout=5)
    try:
        fb.wait(timeout=20)
    except subprocess.TimeoutExpired:
        fb.terminate()
        fb.wait(timeout=5)
    if rec.returncode != 0:
        raise RuntimeError(f"record exit {rec.returncode}: {' | '.join(errs[-8:])}")
    project = next((l for l in reversed(lines) if l and not l.startswith("{")), None)
    if not project or not os.path.isfile(os.path.join(project, "project.json")):
        raise RuntimeError(f"no project folder in the record output: {lines[-3:]}")
    if not os.path.isfile(fb_log):
        raise RuntimeError("flashbeep wrote no log")
    os.makedirs(OUT, exist_ok=True)
    shutil.copy(fb_log, os.path.join(OUT, f"run{n}-flashbeep.json"))
    report = os.path.join(OUT, f"run{n}.json")
    r = subprocess.run([DEVTOOLS, "sync", "analyze", project, "--log", fb_log, "--json", report],
                       capture_output=True, text=True, timeout=900)
    print(r.stdout, end="", flush=True)
    if r.returncode != 0:
        raise RuntimeError(f"analyze exit {r.returncode}: {r.stderr.strip()[-400:]}")
    return report


def main():
    reports, failed = [], 0
    for n in range(1, RUNS + 1):
        log(f"sync: run {n} of {RUNS} ({DURATION_S} s: display, window at 35 s, display at 50 s)")
        try:
            reports.append(one_run(n))
        except Exception as e:
            failed += 1
            log(f"sync: run {n} FAILED: {e}")
    if reports:
        r = subprocess.run([DEVTOOLS, "sync", "summarize", *reports], capture_output=True,
                           text=True)
        print(r.stdout, end="", flush=True)
        if r.returncode != 0:
            log(f"sync: summarize failed: {r.stderr.strip()}")
    log(f"sync: {len(reports)} of {RUNS} runs analyzed; reports in {OUT}")
    return 0 if reports and not failed else 1


def cleanup():
    for p in LIVE:
        if p.poll() is None:
            p.terminate()
    if KEEP:
        log(f"sync: kept recordings in {WORK}")
    else:
        shutil.rmtree(WORK, ignore_errors=True)


if __name__ == "__main__":
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(130))
    rc = 1
    try:
        rc = main()
    finally:
        cleanup()
    sys.exit(rc)
