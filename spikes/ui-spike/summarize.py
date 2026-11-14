"""Summarize results.jsonl (median of the runs) as Markdown tables.

Usage: python3 -I summarize.py [results.jsonl]
"""
import json
import statistics
import sys
from collections import defaultdict

path = sys.argv[1] if len(sys.argv) > 1 else "results.jsonl"
runs = defaultdict(list)
with open(path, encoding="utf-8") as f:
    for line in f:
        line = line.strip()
        if line:
            r = json.loads(line)
            runs[(r["toolkit"], r["backend"], r["mode"])].append(r)


def med(rs, *keys):
    vals = []
    for r in rs:
        v = r
        for k in keys:
            v = v[k]
        vals.append(v)
    return statistics.median(vals)


def mb(kb):
    return f"{kb / 1024:.0f} MB"


order = [("egui", "wgpu"), ("egui", "glow"), ("iced", "wgpu"), ("iced", "tiny-skia")]

print("## Memoria y arranque (mediana)\n")
print("| Toolkit | Renderer | RAM vacía | RAM con datos | Pico | Primer fotograma (vacía / con datos) | CPU en reposo | Fotogramas en 5 s de reposo | Ejecuciones |")
print("|---|---|---|---|---|---|---|---|---|")
for tk, be in order:
    e, f = runs.get((tk, be, "empty"), []), runs.get((tk, be, "full"), [])
    if not e or not f:
        continue
    print(
        f"| {tk} | {be} | {mb(med(e, 'rss_end_kb'))} | {mb(med(f, 'rss_loaded_kb'))} | {mb(med(f, 'hwm_kb'))} "
        f"| {med(e, 'first_frame_ms'):.0f} / {med(f, 'first_frame_ms'):.0f} ms | {med(f, 'idle_cpu_pct'):.1f} % "
        f"| {med(f, 'idle_frames'):.0f} | {len(f)} |"
    )

print("\n## Fluidez con datos (mediana; 16,7 ms = 60 fps, límite del compositor)\n")
print("| Toolkit | Renderer | Escribir: p50 / p95 / máx | Escribir: CPU por fotograma | Árbol: p95 | Editor: p95 | CPU por fotograma al desplazar |")
print("|---|---|---|---|---|---|---|")
for tk, be in order:
    f = runs.get((tk, be, "full"), [])
    if not f:
        continue
    print(
        f"| {tk} | {be} | {med(f, 'typing', 'p50_ms'):.1f} / {med(f, 'typing', 'p95_ms'):.1f} / {med(f, 'typing', 'max_ms'):.0f} ms "
        f"| {med(f, 'typing', 'cpu_ms_per_frame'):.1f} ms | {med(f, 'tree_scroll', 'p95_ms'):.1f} ms "
        f"| {med(f, 'editor_scroll', 'p95_ms'):.1f} ms "
        f"| {med(f, 'tree_scroll', 'cpu_ms_per_frame'):.1f} / {med(f, 'editor_scroll', 'cpu_ms_per_frame'):.1f} ms |"
    )
