"""Launches apifast-gui on a collection, reads its startup timing marks,
waits 10 s idle and reports PSS (from /proc/<pid>/smaps_rollup) and the CPU
used over the next 5 s. Linux only. See docs/10-rendimiento-y-memoria.md.

    cargo build --release -p apifast-gui
    python3 tools/bench/measure.py target/release/apifast-gui /tmp/c100
"""

import os
import signal
import subprocess
import sys
import tempfile
import time

binary, collection = sys.argv[1], sys.argv[2]
env = dict(os.environ, APIFAST_TIMINGS="1",
           APIFAST_DATA_DIR=tempfile.mkdtemp(), APIFAST_CONFIG_DIR=tempfile.mkdtemp())
p = subprocess.Popen([binary, collection], env=env, stderr=subprocess.PIPE, text=True)
os.set_blocking(p.stderr.fileno(), False)
marks = []
deadline = time.time() + 15
while time.time() < deadline and len(marks) < 2:
    line = p.stderr.readline()
    if line.startswith("apifast:"):
        marks.append(line.strip())
    else:
        time.sleep(0.01)
time.sleep(10)
def cpu_ticks():
    f = open(f"/proc/{p.pid}/stat").read().split()
    return int(f[13]) + int(f[14])
t0 = cpu_ticks(); time.sleep(5); t1 = cpu_ticks()
pss = [l for l in open(f"/proc/{p.pid}/smaps_rollup") if l.startswith(("Pss:", "Rss:"))]
p.send_signal(signal.SIGTERM); p.wait(timeout=10)
print(collection.rsplit('/',1)[-1], *marks, *(l.split()[0] + l.split()[1] + "kB" for l in pss),
      f"idle CPU over 5 s: {(t1 - t0) / os.sysconf('SC_CLK_TCK') * 100 / 5:.1f}%", sep=" | ")
