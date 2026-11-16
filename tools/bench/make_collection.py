"""Writes a synthetic collection of N requests, in folders of 50, to
measure apifast with it (docs/10-rendimiento-y-memoria.md).

    python3 tools/bench/make_collection.py /tmp/c100 100
"""

import os
import sys

root, count = sys.argv[1], int(sys.argv[2])
os.makedirs(f"{root}/environments", exist_ok=True)
open(f"{root}/opencollection.yml","w").write("opencollection: 1.0.0\ninfo:\n  name: Bench %d\n" % count)
open(f"{root}/environments/local.yml","w").write("name: local\nvariables:\n  - name: host\n    value: http://127.0.0.1:8765\n")
per = 50
for i in range(count):
    folder = f"{root}/folder{i // per:03d}"
    if i % per == 0:
        os.makedirs(folder, exist_ok=True)
        open(f"{folder}/folder.yml","w").write(f"info:\n  name: Folder {i // per}\n  type: folder\n  seq: {i // per + 1}\n")
    open(f"{folder}/req{i:05d}.yml","w").write(
        f"info:\n  name: Request {i}\n  type: http\n  seq: {i % per + 1}\nhttp:\n  method: GET\n  url: \"{{{{host}}}}/items/{i}?page=1\"\n  headers:\n    - name: Accept\n      value: application/json\n")
