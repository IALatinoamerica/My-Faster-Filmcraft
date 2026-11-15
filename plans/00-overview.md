# Round 3: masked effects on the GPU, and a report of what renders on the CPU

Decided by Bruno on 2026-11-15. Not started. Rounds 1 and 2 are implemented; their plans were
removed (they are in the Git history, last in commit `5efbb67f`) and their numbers stay in
[results.md](results.md).

## Why

Round 2 found that one small detail (adjustment layers 10 px off centre) made 24 % of a video
render on the CPU, and nothing in the app said so: it took a benchmark sweep to find it. Two
things follow.

1. **An effect with a mask of its own still renders on the CPU.** On a media clip that costs
   that clip's layer; on an adjustment layer it costs the whole frame, for as long as the layer
   is on screen. Bruno's agents edit his projects, and one mask on one effect is enough to bring
   the stutter back.
2. **Nothing reports what renders on the CPU.** Whatever is left on the CPU after stage 1
   (effects with no GPU version, HDR, titles with effects…) should be named, with track, clip,
   time and reason, to Bruno in the app and to an agent through a command.

Both ideas exist in Bruno's other app, Filmcraft Exporter
(`/media/bruno/SSD 500 2 Linux/Software/Filmcraft Exporter`, commits `7bc4dda8`, `a4dee28f`
and `455207bc`; its `crates/render/src/slow_paths.rs`). That code is a reference for behaviour
and wording only. It is built on FilmCraft 0.19 and has a different effect stage, so nothing is
copied: each stage is written for this fork's code.

## Stages

| Plan | What | Result for Bruno |
|---|---|---|
| [01](01-masked-effects.md) | Effects limited by a mask of their own, on media clips and on adjustment layers | A mask on Color Grading (or any GPU effect) no longer slows playback |
| [02](02-cpu-report.md) | The report: a function in `crates/render` and a command | An agent can ask "what renders on the CPU in this sequence?" and get a list |
| [03](03-cpu-report-window.md) | The report in the interface | Bruno sees the list in the app and jumps to each clip |

Stage 1 goes first so that the report is written once, against the final coverage.

## Left out, by earlier decision

Audio mixing on its own thread, pre-decoding at cuts, HDR sequences, blend modes other than
Normal on adjustment layers, effects that have no GPU version. Stage 2 reports the last three
instead of fixing them.

## How every stage is checked

1. The CPU renderer stays the reference. A new GPU op has a CPU `apply` that reproduces the
   existing CPU result (test in `crates/render`) and a GPU test in `crates/gpu/src/fx_tests.rs`
   that compares the shader with it on the real device.
2. A plan test: the case that planned for the CPU now plans for the GPU; hostile parameters are
   declined or bounded, never a panic (AGENTS.md §0).
3. `cargo fmt`, then, always with `--release` (a debug build would add a second build folder):
   `cargo clippy --release -p filmcraft-render -p filmcraft-gpu -p filmcraft-ui-egui --all-targets -- -D warnings`
   and `cargo test --release -p filmcraft-render -p filmcraft-gpu`, plus
   `cargo test --release -p filmcraft-ui-egui --lib` (and `-p filmcraft-engine` when a command
   is added).
4. Where a stage says so, the benchmark (`UPDATING.md` section 5 explains how to build and run
   `bench_playback`); numbers go to [results.md](results.md).
5. One local commit per stage on `main`. No push.
6. After the last stage: add the new rows to "What must survive" in `UPDATING.md`, correct its
   list of what still renders on the CPU and its list of upstream tests changed on purpose, and
   install following its section 7.

A mistake in `crates/gpu/src/fx.wgsl` (even a reserved word used as a name) disables the whole
effect stage without any message; the GPU tests then fail with "effect image". Op codes 100–118
are taken; new ones start at 119 and must match in `fx.rs` and `fx.wgsl`.
