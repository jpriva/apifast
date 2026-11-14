#!/usr/bin/env bash
# Runs every toolkit/backend/mode combination N times and appends JSON lines to results.jsonl.
# Keep the mouse pointer outside the spike windows while it runs.
set -u
cd "$(dirname "$0")"
N=${N:-3}
OUT=${OUT:-results.jsonl}
[ "${APPEND:-0}" = 1 ] || : > "$OUT"
for i in $(seq 1 "$N"); do
  for mode in empty full; do
    SPIKE_SCENARIO=1 SPIKE_OUT=$OUT SPIKE_MODE=$mode SPIKE_BACKEND=wgpu timeout 180 ./target/release/egui-proto >/dev/null 2>&1
    SPIKE_SCENARIO=1 SPIKE_OUT=$OUT SPIKE_MODE=$mode SPIKE_BACKEND=glow timeout 180 ./target/release/egui-proto >/dev/null 2>&1
    SPIKE_SCENARIO=1 SPIKE_OUT=$OUT SPIKE_MODE=$mode SPIKE_BACKEND=wgpu ICED_BACKEND=wgpu timeout 180 ./target/release/iced-proto >/dev/null 2>&1
    SPIKE_SCENARIO=1 SPIKE_OUT=$OUT SPIKE_MODE=$mode SPIKE_BACKEND=tiny-skia ICED_BACKEND=tiny-skia timeout 180 ./target/release/iced-proto >/dev/null 2>&1
  done
done
wc -l "$OUT"
