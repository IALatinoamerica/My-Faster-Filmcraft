# 00 — Baseline measurement (prerequisite, not an improvement)

The ranking of plans 01–05 comes from reading the code and the two test projects. Nothing was
built or run. This step turns the estimates into numbers and must run before any change.

## Order of work

File numbers 01–05 rank the original areas by expected gain on the test projects. Plans 06 and 07
were added later for effects the test projects do not use. The order to carry them out:

1. 00 baseline
2. 01 opacity masks on the GPU
3. 02a, then 02b (Transform with its own shutter angle)
4. 06 safety net (CPU effects, GPU placement)
5. 07 effects batch: Luma Key, Rounded Crop, Drop Shadow, Noise, Ultra Key 7a, 7b
6. 03a, then 03b (seek latency)

Not planned for now: 03c, 04, 05.

Plans 06 and 07 need a synthetic case to measure: a copy of `SHORT TEST.fcproj` with the effect
under test on the camera clip and on an adjustment layer. Keep the copy outside the repository.

## Machine

| Part | Value |
|---|---|
| CPU | AMD Ryzen 9 5900X, 12 cores / 24 threads, AVX2 |
| RAM | 62 GB, no swap |
| GPU | AMD Radeon RX 6700 XT (Navi 22), `amdgpu`, Mesa 25.2.8 |
| Vulkan | RADV, API 1.4 (what wgpu uses) |
| VA-API | `radeonsi`, libva 2.12: H.264 CBP/Main/High, HEVC Main/Main 10, VP9 0/2, AV1 0 decode |
| Session | X11 |
| Disk | 79 GB free on the project drive |

## Workload (`FULL TEST.fcproj`, sequence `EDIT_PR_277 News Nov 02`)

- 1920×1080 at 60 fps, 27 min, 5 video tracks in use.
- Media: 140 HEVC Main 8-bit 1080p60 files (OBS; GOP 120 frames, no B-frames, 0.5–6 Mbit/s), a few
  H.264 files, one 2160p H.264.
- Typical frame: screen recording (Video 1) + camera (Video 3, scaled to 51.5 %, circular opacity
  mask) + up to two adjustment layers (Transform zooms, Gaussian Blur).

## What to record

1. Build once: `cargo build --release -p filmcraft -p filmcraft-cli` (into `target/`; several GB).
2. Open `FULL TEST.fcproj` with `cargo run --release -p filmcraft -- --control 9876`.
3. Read `perf.stats` before and after each run and diff (counters are cumulative):
   - `decode.hardware`: `frames` vs `softwareFrames`, `declined`, `fallbacks`. This answers "is the
     GPU decoding?". The VA-API path was measured upstream on Intel only; on `radeonsi` a
     mid-stream error falls back to software without a message in the UI.
   - `frames.decodeMs`, `frames.renderMs` (p50 / p95): `renderMs` is the CPU work plans 01 and 02
     remove.
   - `playback.shown` / `dropped` / `dropRate`.
   - `ui.fps`, `process` CPU seconds.
4. Three fixed runs, each at Full and 1/2 resolution:
   - **Play A**: 20 s of a plain section (screen + camera, no adjustment layer).
   - **Play B**: 20 s across a Transform zoom with its own shutter angle.
   - **Scrub**: a scripted playhead sweep (set the playhead to 40 positions 3 s apart, wait for the
     exact frame each time; record time to frame from `monitor.frame` reaching `playheadFrame`).
5. Save the numbers in `plans/results-baseline.md`. Every later plan repeats the same three runs.

## Decision gates

- `softwareFrames` > 0 for HEVC: fix that first; it outranks everything below.
- Play A `renderMs` p50 under 3 ms: plan 01's estimate is wrong, re-rank.
- Dropped frames concentrated at cuts: plan 03 moves up.
