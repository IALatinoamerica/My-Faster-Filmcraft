# Results, round 2

`bench_playback --hw auto --scenario project --project "FULL TEST 02.fcproj" --sequence
"EDIT_PR_282 News Nov 02" --start <s> --seconds 10 --res full`: frames shown of 600 due.
FilmCraft was open on the same machine during the baseline sweep.

| Window (start) | What is there | Before | After 01 | After 02 |
|---|---|---|---|---|
| 122 s | zoom layer nudged 10 px | 3 | 570 | |
| 140 s | same, whole window | 0 | 558 | |
| 220 s | same, whole window | 0 | 556 | |
| 1200 s | same, whole window | 0 | 572 | |
| 280 s | zoom layer + Lens Distortion transition | 302 | 526 | 583 |
| 720 s | zoom layer + Lens Distortion transition | 11 | 547 | 583 |
| 690 s | Lens Distortion transition, no zoom layer | 488 | | 563 |
| 1020 s | Lens Distortion transition, no zoom layer | 512 | | 568 |

CPU per due frame in the first four windows after 01: 9.1–9.4 ms.

## After 03 (Emboss, VR Digital Glitch)

| Window (start) | Before | After |
|---|---|---|
| 380 s | 544 | 569 |
| 480 s | 540 | 560 |
| 1060 s | 539 | 552 |

## After 04–06 (Color Grading)

Two copies of the project with one Color Grading that uses every section (Look LUT, wheels,
curves, HSL Secondary with Denoise and Blur, Sharpen): `FX Color Grading CAM.fcproj` (on the 247
camera clips of Video 3) and `FX Color Grading ADJ.fcproj` (on the 16 zoom layers of Video 2).
Not measured before the change (the whole effect rendered on the CPU then).

| Project | Window (start) | Frames shown of 600 | GPU pass per frame (p50) |
|---|---|---|---|
| CAM | 30 s | 582 | 12.3 ms |
| CAM | 140 s | 559 | 12.0 ms |
| ADJ | 30 s (no graded layer in the window) | 583 | 2.0 ms |
| ADJ | 140 s | 561 | 11.8 ms |

A full grade costs about 10 ms of the 16.7 ms a frame has at 60 fps on this GPU: it fits, with
little room for a second heavy effect on the same frame at Full resolution.

## The whole timeline, before and after

136 windows of 10 s over `FULL TEST 02.fcproj` (the last one is 7.5 s long and left out of the
figures), 600 frames due in each. FilmCraft was open during the first sweep and closed or idle
during the second.

| | Before | After 01–06 |
|---|---|---|
| Frames shown, mean per window | 431 | 571 |
| Lowest window | 0 | 528 |
| Windows with no frame shown | 19 | 0 |
| Windows under 500 frames | 45 | 0 |
| Frames shown in total (of 81 000) | 58 146 | 77 128 |

What is left (about 29 frames per window) falls at cuts, where each decoder starts again from a
keyframe: left out of this round by decision.

## Checks

Each stage: `cargo fmt`; clippy with warnings as errors on `filmcraft-render`, `filmcraft-gpu`
and `filmcraft-ui-egui`; the tests of `filmcraft-render` and `filmcraft-gpu` and the unit tests
of `filmcraft-ui-egui`, all passing. Not run: `cargo xtask ci` (the whole workspace and the web
build). The audio was not measured: its crackling was explained from the code as a consequence of
the CPU being saturated, which no longer happens in this project.

## Round 3, stage 1: effects limited by a mask of their own

Two more copies of the project: `FX Color Grading Masked CAM.fcproj` and
`FX Color Grading Masked ADJ.fcproj`, the projects above with a feathered elliptical mask on each
Color Grading (247 camera clips, 16 zoom layers). Frames shown of 600 in a 10 s window, Full
resolution, 2026-11-15. FilmCraft was open during the "before" runs and closed for the "after"
runs.

| Project | Window (start) | Before | After | Unmasked project, same run |
|---|---|---|---|---|
| Masked CAM | 30 s | 0 | 578 | 584 |
| Masked CAM | 140 s | 0 | 562 | 554 |
| Masked ADJ | 30 s (no graded layer in the window) | 574 | 575 | 583 |
| Masked ADJ | 140 s | 0 | 555 | 558 |

Before, a masked effect sent the clip's layer (CAM) or the whole frame (ADJ) to the CPU: 235 and
208 ms of CPU per frame due. After, the GPU pass costs the same as without the mask (p50 12.3 ms
masked against 12.6 ms unmasked on CAM at 30 s).

Checks run: `cargo fmt`; clippy with warnings as errors on `filmcraft-render`, `filmcraft-gpu` and
`filmcraft-ui-egui`; tests of `filmcraft-render` (286 pass), `filmcraft-gpu` (64 pass, 4 ignored)
and `filmcraft-ui-egui --lib` (358 pass). No upstream test asserted the old behaviour, so none
was changed. Not run: `cargo xtask ci`; no visual check in the app.

## Round 3, stage 2: the report of what renders on the CPU

`sequence.cpuReport` (engine command; `filmcraft_render::slow_paths`), run read-only through
`filmcraft-cli --project <file> exec sequence.cpuReport` on 2026-11-15. It decodes nothing; each
run took under 0.1 s including opening the project.

| Project, sequence | Entries | Whole-frame | What it lists |
|---|---|---|---|
| `FULL TEST 02`, `EDIT_PR_282 News Nov 02` | 0 | 0 | nothing: the GPU draws the whole sequence |
| `FULL TEST 02`, the sequence the project opens on | 4 | 0 | 4 titles with effects or opacity masks (track 4) |
| `FX Color Grading Masked ADJ`, `EDIT_PR_282 News Nov 02` | 0 | 0 | nothing (stage 1) |
| `FX Twirl ADJ`, the sequence it opens on | 2 | 1 | the adjustment layer with Twirl (track 6, whole frame); 1 title |
| `FX Twirl CAM`, the sequence it opens on | 56 | 0 | 55 clips with Twirl (track 3); 1 title |

Checks run: `cargo fmt`; clippy with warnings as errors on `filmcraft-render`, `filmcraft-gpu`,
`filmcraft-engine` and `filmcraft-ui-egui`; tests of `filmcraft-render` (289 pass),
`filmcraft-gpu` (64 pass), `filmcraft-ui-egui --lib` (358 pass) and `filmcraft-engine --lib`
(905 pass, 1 fails: `every_builtin_preset_exports_and_ffprobe_confirms`, at "APV 422-10"; it
fails the same way without this stage's changes, the local `ffprobe` does not report that codec).
Not run: `cargo xtask ci`.

## Round 3, stage 3: the render bar asks the planner

`FULL TEST 02.fcproj`, sequence `EDIT_PR_282 News Nov 02` (328 segments), read with
`sequence.renderBar` on 2026-11-15:

| | Before | After |
|---|---|---|
| Red segments | 129 (342.1 s) | 6 (0.7 s) |
| Yellow segments | 199 (1015.4 s) | 322 (1356.8 s) |
| Time to compute the bar (`render_bar_timing`, ignored test) | 5.72 ms | 7.27 ms |

The six red segments left are three short transitions (26.5 ms estimated against a 25 ms budget).
The benchmark of round 2 showed 528 frames or more of 600 in every 10 s window of this sequence,
so "yellow almost everywhere" agrees with it.

In the running app (control channel, a copy of `FX Twirl ADJ.fcproj`): `sequence.cpuReport`
answered with `"device": "actual"` and listed the Twirl adjustment layer (whole frame) and one
title, and the bar over that layer was red. The tooltip on the bar was not looked at on screen;
its text is covered by a test.

A list window (`Sequence ▸ CPU Report…`) was built, seen working, and removed at Bruno's
decision: the bar is the warning.

Checks run: `cargo fmt`; clippy with warnings as errors on `filmcraft-render`, `filmcraft-gpu`,
`filmcraft-engine` and `filmcraft-ui-egui`; tests of `filmcraft-render` (290 pass),
`filmcraft-gpu` (64 pass), `filmcraft-engine --lib` (905 pass, the APV export test fails as
before) and `filmcraft-ui-egui --lib` (359 pass). After the window was removed only clippy and
the `filmcraft-ui-egui` tests were run again. Not run: `cargo xtask ci`; the benchmark was not
repeated after this stage (the change runs only when the project changes, not during playback).

## Round 4: a file shown twice at once, and the render bar over stacked clips

Decided and implemented on 2026-11-19, without a written plan. Project
`285 Gemini 4 NEON - Segundo corte.fcproj`, sequence `04 Segundo corte` (60 fps), run on a copy.

**Cause of the stall at 00:01:30:19.** `sequence.cpuReport` listed nothing: the GPU drew every
layer. The same screen recording was on Video 1 and, 54 s further into it, on Video 2. One
source per media item meant one decoder for both: it restarted at a keyframe at every request
(82 restarts and 2 494 samples decoded in 4 s). Each such clip now reads a copy of its own
(`crates/render/src/readers.rs`). It was the only such place in this sequence.

`bench_playback --scenario project --res full`, frames shown, FilmCraft closed:

| Start | Length | Before | After |
|---|---|---|---|
| 88 s | 4 s | 92 of 240 | 211 of 240 |
| 90 s | 4 s | 5 of 240 | 203 of 240 |
| 92 s | 4 s | 127 of 240 | 192 of 240 |
| 86 s | 10 s | | 563 of 600 |
| 80 s | 10 s | 219 of 240 (4 s) | 570 of 600 |
| 96 s | 10 s | 219 of 240 (4 s) | 587 of 600 |
| 300 s | 10 s | | 574 of 600 |

**Render bar.** The bar was still red there: 28 ms against 25. Three HEVC layers had been
counted at 6.5 ms each, one after another, and each title's text layer as a GPU effect. Now the
decodes count once, as the slowest (they run side by side), and text layers are not effects.

| `sequence.renderBar` | Before | After |
|---|---|---|
| Red segments | 26 (8.5 s) | 0 |
| Highest segment cost | 30.0 ms | 23.5 ms |
| Time to compute the bar (`render_bar_timing`) | | 6.59 ms |

Playback of stretches that were red, 10 s windows: 0 s 563, 184 s 504, 280 s 512 of 600. An
ordinary stretch that was never red, 200 s, shows 521: the red ones play like the rest, whose
small losses fall at cuts.

Checks run: `cargo fmt`; clippy with warnings as errors on `filmcraft-render`, `filmcraft-gpu`,
`filmcraft-engine` and `filmcraft-ui-egui`; tests of `filmcraft-render` (294 pass), `filmcraft-gpu`
(64 pass), `filmcraft-codecs` (all pass), `filmcraft-ui-egui --lib` and `filmcraft-engine --lib`
(the APV export test fails as before; counts in the last run below). Each new test failed
without its change; the monitors' test was checked by removing `JobProvider::reader_source`. Not run: `cargo xtask ci`. Bruno checked playback in the running app; the corrected bar was
reopened for him but not yet looked at.

Last run of the checks of `UPDATING.md` section 4 (2026-11-19, after the tests of the copies
were added): clippy with warnings as errors on all six crates it lists, clean; tests of
`filmcraft-render` (294), `filmcraft-gpu` (64), `filmcraft-codecs` and `filmcraft-platform` all
pass, `filmcraft-ui-egui --lib` 360 pass, `filmcraft-engine --lib` 908 pass and only the known
APV export test fails.
