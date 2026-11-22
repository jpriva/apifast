"""PSS of apifast-gui by mapping (its own code, heap, each library) after
10 s idle: where the memory goes when a budget breaks. Linux only.

The RAM budgets of docs/10 are for apifast's own memory: everything but the
system's graphics stack (Mesa and its LLVM, the Vulkan and EGL drivers and
loaders), which depends on the machine and is reported apart.

    python3 tools/bench/breakdown.py target/release/apifast-gui [collection]
"""

import collections
import os
import re
import signal
import subprocess
import sys
import tempfile
import time

binary = sys.argv[1]
args = [binary] + sys.argv[2:3]
env = dict(os.environ, APIFAST_DATA_DIR=tempfile.mkdtemp(), APIFAST_CONFIG_DIR=tempfile.mkdtemp())
p = subprocess.Popen(args, env=env, stderr=subprocess.DEVNULL)
time.sleep(10)
by = collections.Counter(); name = None; total = 0
for line in open(f"/proc/{p.pid}/smaps"):
    parts = line.split()
    if parts and '-' in parts[0] and len(parts) >= 5 and ':' not in parts[0]:
        name = parts[5] if len(parts) > 5 else "[anon]"
        name = os.path.basename(name) if name.startswith('/') else name
    elif line.startswith("Pss:"):
        kb = int(parts[1]); by[name] += kb; total += kb
p.send_signal(signal.SIGTERM); p.wait(timeout=10)
# The system's graphics stack: what the GPU driver loads, not apifast.
GRAPHICS = re.compile(r"^(libLLVM|libvulkan|libSPIRV|libgallium|libEGL|libGLX|libGL|libGLES|libgbm|libdrm|libglapi|libdisplay-info|libelf|libsensors|libexpat|libz3)|_dri\.so|_icd")
graphics = sum(kb for name, kb in by.items() if GRAPHICS.search(name))
print(f"total PSS {total/1024:.1f} MB: apifast {(total - graphics)/1024:.1f} MB, graphics drivers {graphics/1024:.1f} MB")
for name, kb in by.most_common(12):
    tag = "  (graphics)" if GRAPHICS.search(name) else ""
    print(f"{kb/1024:7.1f} MB  {name}{tag}")
