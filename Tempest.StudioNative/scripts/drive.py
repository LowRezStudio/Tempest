#!/usr/bin/env python3
"""Drive the native Studio for screenshots: start it with TEMPEST_DRIVE=1,
send steps (one per line of a script file or stdin), wait for each `ok`,
and write window-only screenshots on `shot <name>` lines.

    scripts/drive.py <steps.txt> <out dir> [game dir]

Step lines are the app's (see src/driver.rs), plus `shot <name>` (a PNG of
the app's window only, never the whole screen) and `# comments`.
"""
import os, subprocess, sys, time

here = os.path.dirname(os.path.abspath(__file__))
root = os.path.dirname(here)
steps = open(sys.argv[1]).read().splitlines() if len(sys.argv) > 1 and sys.argv[1] != "-" else sys.stdin.read().splitlines()
out = sys.argv[2] if len(sys.argv) > 2 else "."
os.makedirs(out, exist_ok=True)
exe = os.environ.get("STUDIO_EXE", os.path.join(root, "target", "debug", "tempest-studio"))
args = [exe] + sys.argv[3:4]
env = dict(os.environ, TEMPEST_DRIVE="1")
app = subprocess.Popen(args, stdin=subprocess.PIPE, stdout=subprocess.PIPE, env=env, text=True, bufsize=1)

def window_id():
    """The app's largest on-screen window (CGWindowList), as `screencapture -l` wants it."""
    swift = os.path.join(here, "winid.swift")
    r = subprocess.run(["swift", swift, str(app.pid)], capture_output=True, text=True)
    return r.stdout.strip()

def send(step):
    app.stdin.write(step + "\n")
    app.stdin.flush()
    while True:
        line = app.stdout.readline()
        if not line:
            raise SystemExit("the app exited")
        line = line.strip()
        if line == "ok" or line.startswith("err"):
            return line

time.sleep(float(os.environ.get("STUDIO_START_WAIT", "4")))
wid = None
failed = 0
for step in steps:
    step = step.strip()
    if not step or step.startswith("#"):
        continue
    if step.startswith("shot "):
        wid = wid or window_id()
        path = os.path.join(out, step[5:].strip() + ".png")
        subprocess.run(["screencapture", "-x", "-o", "-l", wid, path])
        print("shot", path)
        continue
    r = send(step)
    print(step, "->", r)
    failed += r != "ok"
app.terminate()
sys.exit(1 if failed else 0)
