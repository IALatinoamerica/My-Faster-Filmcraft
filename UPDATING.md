# Updating this fork to a new FilmCraft release

This repository is Bruno's personal fork of the FilmCraft video editor. It is FilmCraft plus a
small set of changes that make timeline playback and scrubbing fast on his machine. Every time
FilmCraft publishes a release, the fork is updated to it. This file is the procedure.

You need no other knowledge of the project to carry it out. Follow [AGENTS.md](AGENTS.md) for
the coding rules; explicit instructions from Bruno for the task take precedence over this file.

## Owner's update policy

**Take everything from the release. Keep our optimizations working.**

- Unlike a selective port, the whole release is merged. There is no review of whether the update
  is "needed": it always is.
- Our optimizations stay, **even when FilmCraft has optimized the same area itself**. Where the
  two overlap, our version keeps doing the work it does today. Upstream's version is not adopted
  in its place because it is newer, and ours is not dropped because upstream now has something
  similar.
- Everything outside our optimizations is upstream's: take it as it comes, with no local changes.
  This keeps the next update small.
- An update is finished only when the benchmark in section 5 shows the speed is still there
  **and** the result is installed on the system (section 7), so that Bruno can open FilmCraft
  from his menu and work. A merge that compiles and passes tests but plays slowly has lost the
  work, and a merge that is not installed has not reached him.

If keeping one of our optimizations is impossible without changing what it does (upstream removed
or redesigned the code it stands on), stop before that change, explain what upstream changed and
what you propose, and wait for Bruno's answer. Do not delete an optimization to finish a merge.

## What "update" authorizes

An instruction to update authorizes: fetching from upstream, merging the release into local
`main`, resolving conflicts, building, running the checks, committing the merge locally, and
installing the result as the system's `filmcraft` package (section 7). The installation needs
Bruno's password; he types it himself when asked.

It does not authorize: pushing anywhere, installing any other software or build tool, editing
Bruno's launcher files (see section 7), or changing code for any reason other than making the
merge work.

## The repository

| Thing | Value |
|---|---|
| `origin` | Bruno's fork on GitHub. Never push unless he asks. |
| `upstream` | `https://github.com/storytold/filmcraft.git`, the official project. Never push to it. |
| Releases | Tags `vX.Y.Z` on upstream. They are cut from a release branch, so a release tag is usually **not** an ancestor of upstream `main`. Merge the tag, not `main`. |
| Version shown by the app | `version` in the root `Cargo.toml`. It comes with the merge; never edit it by hand. |
| Where the code lives | This checkout. It stays here; installing copies the built programs to the system. |
| How Bruno runs the fork | The system package `filmcraft` (`/usr/bin/filmcraft`, `/usr/bin/filmcraft-cli`), built from this checkout and installed in section 7. Its package version ends in `+fork.<commit>`. |
| Restore points | A tag `pre-<version>-merge` made before each update. |

The app shows an "update available" notice when the `version` it was built with is older than
the latest release. That notice is the usual reason Bruno asks for an update. Its "Download"
button fetches the official build, which would replace the fork and lose our optimizations: it
is never used. The update is done here instead.

## What must survive: our optimizations

List our commits at any time with `git log --oneline --no-merges upstream/release..main`
(commits on `main` that are in no upstream release). The designs are described in `plans/`, and
the measured results in `plans/results.md`. In short:

| Optimization | What it does | Where it lives |
|---|---|---|
| Opacity masks on the GPU | A media clip with an opacity mask no longer renders on the CPU. | `crates/render/src/plan.rs` (`opacity_mask_op`, `MASK_PLANES`), `FxOp::Mask` in `crates/render/src/gpufx.rs`, `OP_MASK` in `crates/gpu/src/fx.rs` and `fx.wgsl` |
| Transform with its own shutter angle on the GPU | An adjustment layer with such a Transform no longer sends the whole frame to the CPU. | `FxOp::ResampleBlur`, `transform_resample`, `SHUTTER_SAMPLES` in `gpufx.rs`; `OP_RESAMPLE_ACC` in `fx.rs` / `fx.wgsl`; `crates/render/src/vfx.rs` |
| Five effects on the GPU | Luma Key, Rounded Crop, Drop Shadow, Noise (and legacy Noise), Ultra Key. | `crates/render/src/gpufx_more.rs` (ours entirely), their entries in `GPU_EFFECTS` in `gpufx.rs`, op codes 102–109 in `fx.rs` / `fx.wgsl` |
| Adjustment layers that keep part of the picture below | A layer moved or scaled by Motion, not opaque or with an opacity mask no longer sends the whole frame to the CPU (a 10 px nudge used to). | `adjustment_weight` in `crates/render/src/lib.rs`; `adjustment_ops`, `adjustment_plane`, `ADJUSTMENT_PLANES` and the `Adjust` arm of `execute_cpu` in `plan.rs`; `FxOp::MixBase` and `mix_base` in `gpufx.rs`; `OP_MIX_BASE`, the kept base texture (`mix_base`) and the table binding (binding 4, `Step::table`) in `fx.rs` / `fx.wgsl` |
| Three more effects on the GPU | Lens Distortion, Emboss, VR Digital Glitch (monoscopic). | `LensDistortion`, `Emboss`, `DigitalGlitch` in `gpufx_more.rs`; their entries in `GPU_EFFECTS` and `FxOp::eval`; op codes 111–113 |
| All of Color Grading on the GPU | Looks, Color Wheels, Curves, Input LUT, Look LUT, HSL Secondary and Sharpen (upstream runs only Basic, Creative sliders and Vignette there). | `grade`, `GradeAdvanced`, `GradeLut`, `GradeHsl` in `gpufx_more.rs`; `FxOp::Seq` and the tail of the `"lumetri"` arm of `FxOp::eval` in `gpufx.rs`; `lut_step` and op codes 114–118 in `fx.rs` / `fx.wgsl`; `boxes_for_gauss`, `median_filter`, `blur_plane` made `pub(crate)` in `effects.rs` |
| GPU placement of clips with a CPU-only effect | Such a media clip is uploaded at its own size, not as a full canvas. | `Unplaced`, `item_layer_unplaced` in `crates/render/src/lib.rs`; the CPU branch of `push_item` in `plan.rs` |
| Sources fetched side by side | The clips of one frame are decoded at the same time. | `prefetch_sources`, `media_requests` in `plan.rs`; the call in `crates/ui-egui/src/frames.rs` |
| No read-back of pictures nobody shows | After a jump, the hardware decoder does not copy back pictures before the wanted one. | `set_output_from` in `crates/codecs/src/video.rs`; `RETURN_BEFORE` in `crates/codecs/src/gop.rs`; `crates/platform/src/vaapi/{h264,hevc}.rs`, `vaapi.rs`, `hybrid.rs` |
| Benchmark | `--project`, `--sequence`, `--start`, scenarios `project` and `project-seek`. | `crates/ui-egui/examples/bench/playback.rs` |
| App icon | A gold owl, so the fork is told apart from the installed FilmCraft. | `assets/app-icon/hicolor/*/apps/ai.storyteller.filmcraft.png` |

Each optimization has tests that compare it with the CPU renderer or count the work done. They
are part of the optimization: keep them, and fix them when upstream changes something they use.
Three upstream tests were changed on purpose, because they asserted the old slow behaviour:

- `transform_with_its_own_shutter_angle_runs_on_the_gpu_unless_it_shrinks` in `gpufx.rs`;
- `an_adjustment_layer_the_gpu_cant_run_plans_the_frame_on_the_cpu` in
  `crates/render/src/adjustment_tests.rs`: its shutter case shrinks the frame, its CPU-only effect
  is Twirl instead of Emboss, and its "opacity 50 %" and "Motion at 50 %" cases are gone (they run
  on the GPU now);
- `lumetri_unsupported_sections_fall_back` in `crates/gpu/src/fx_tests.rs`: only HDR and a Denoise
  window over 3 pixels still fall back.

If upstream edits those tests, keep our expectation.

What still renders on the CPU, by decision: an effect with a mask of its own, blend modes other
than Normal on an adjustment layer, HDR sequences, and effects with no GPU version (Twirl and
others). An effect upstream adds later has no GPU version here until one is written.

A mistake in `fx.wgsl` (even a reserved word used as a name) makes the whole effect stage fail
to start, and every effect then renders on the CPU without any error on screen. The GPU tests in
`crates/gpu` catch it: they fail with "effect image".

## 1. Before touching anything

1. `git status`. If the working tree has uncommitted changes, stop and ask Bruno. Do not stash,
   reset or commit his work to make room.
2. Check that FilmCraft is not running (`pgrep -a -x filmcraft`). If it is, ask Bruno to save
   and close it; it may hold unsaved work, and it must be closed for the installation. Never
   kill it.
3. Identify the target release. Use the one Bruno names. If he says "the latest", run
   `git fetch upstream --tags` and take the highest stable `vX.Y.Z` tag, then confirm on the
   official release page that it is a published release and not a draft or pre-release.
4. Check ancestry: `git merge-base --is-ancestor <last merged tag> <target tag>` should succeed.
   If it does not, the target is on another release line: report it and ask before merging.
5. Record the starting point: `git tag pre-<version>-merge` (for example `pre-0.21.0-merge`).

The disk is nearly full. Build only into this checkout's `target/`. Do not set
`CARGO_TARGET_DIR`, make another clone or worktree, or leave temporary folders behind.

## 2. Merge the release

```sh
git merge --no-edit vX.Y.Z
```

Use a merge, not a rebase: our commits keep their identity, and the next update only has to deal
with what changed since this one.

Bruno may have GitHub Desktop open. It shows a "Resolve conflicts before Merge" window while the
merge is in progress. Tell him not to click "Continue merge" or "Abort merge"; the commit is made
from here once the checks pass.

## 3. Resolve conflicts

A conflict means upstream changed lines next to ours. The rule for each one:

- **Both sides added something** (two sets of tests at the end of a file, two statements at the
  same spot): keep both. This was every conflict in the 0.20.1 update.
- **Upstream changed code our optimization calls or wraps:** keep our structure and bring
  upstream's change into it. Example from 0.20.1: upstream changed a line in the function we had
  split into `item_layer_unplaced`; the fix was to apply upstream's one-line change inside our
  version, not to take upstream's whole function back.
- **Upstream now does the same thing we do** (its own GPU version of one of our effects, its own
  mask path, its own seek change): keep ours as the path that runs, per the policy above. Then
  make sure the two do not collide; see the next list.
- **A file we never touched:** upstream's version, always.

After a merge, conflict or not, check these collisions by hand, because Git cannot see them:

- **Op code numbers.** Each GPU op has a number that must be the same in `crates/gpu/src/fx.rs`
  and `crates/gpu/src/fx.wgsl` and unique in both. Ours are 100–118. If upstream's numbering
  reaches that range, renumber ours in both files.
- **`GPU_EFFECTS` and the `match` in `FxOp::eval`** (`gpufx.rs`): an effect name must appear once.
  If upstream added one of ours, remove upstream's entry and arm so that ours runs, and say so in
  the report.
- **Struct fields.** Upstream adds fields to structs we build by hand, mainly `effects::FxCtx`.
  The compiler reports each place (`missing field`); copy the value from the surrounding context
  as upstream does in the neighbouring code. In 0.20.1 this was `mask_scale`, in two places.
- **Trait `VideoDecoder`** (`crates/codecs/src/video.rs`): our method `set_output_from` has a
  default body. A new upstream decoder needs nothing; a new upstream *wrapper* around a decoder
  must forward the call, as `hybrid.rs` does, or the seek gain is lost without any error.

Do not use `unwrap`, `expect`, `panic!` or `unsafe` in a resolution (see AGENTS.md §0).

## 4. Build and check

All commands run from the repository root, in release mode, into `target/`.

```sh
cargo fmt --all -- --check
cargo build --release -p filmcraft -p filmcraft-cli
cargo build --release -p filmcraft-ui-egui --example bench_playback
cargo test --release -p filmcraft-render -p filmcraft-gpu -p filmcraft-codecs -p filmcraft-platform
cargo test --release -p filmcraft-ui-egui --lib
cargo clippy --release -p filmcraft-render -p filmcraft-gpu -p filmcraft-codecs -p filmcraft-platform -p filmcraft-ui-egui --all-targets -- -D warnings
```

The test build compiles code the app build does not, so a second round of `missing field`
errors in test helpers is normal; fix them the same way.

Every test must pass. A failing test in one of our areas means the merge broke an optimization
or upstream changed the picture it is compared with: find out which before changing the test.
Do not loosen a tolerance to make a test pass. A failing test in an area we never touched should
be checked against a clean copy of the release tag before it is set aside, and reported.

These are the checks the 0.20.1 update ran. The project's full gate, `cargo xtask ci`, was not
run then; run it if Bruno asks, and report which checks ran either way.

## 5. Confirm the speed is still there

This step decides whether the update kept the optimizations. Close other heavy programs first;
do not build while it runs.

The test projects are outside the repository, in
`/media/bruno/SSD 500 2 Linux/Software/Exporter test project/`. Never save or modify them.

```sh
P="/media/bruno/SSD 500 2 Linux/Software/Exporter test project/FULL TEST.fcproj"
B=target/release/examples/bench_playback
S="EDIT_PR_277 News Nov 02"
for start in 0 815 60; do
  "$B" --hw auto --scenario project --project "$P" --sequence "$S" --start $start --seconds 10 --res full
done
"$B" --hw auto --scenario project-seek --project "$P" --sequence "$S"
```

Read the `shown` column of each `project full play` row (frames shown out of 600 due), the
`lat50` / `lat95` columns of the `project-seek full seek` row (milliseconds to show a frame after
a random jump), and the `hardware decoding:` line.

| Run | What it exercises | After 0.20.1 | Without our optimizations |
|---|---|---|---|
| Play, start 0 | masked camera clip | 581 | 1 |
| Play, start 815 | Transform with its own shutter angle | 597 | 0 |
| Play, start 60 | Transform without it | 573 | 1 |
| Seek, `lat50` / `lat95` | jump latency | 291 / 441 ms | 606 / 907 ms |
| `hardware decoding:` | GPU decoding in use | 0 declined, 0 fallbacks | the same |

Runs vary by a few frames and a few tens of milliseconds. A play run that falls to a handful of
frames means that path went back to the CPU: one of the first three optimizations in the table of
section "What must survive" was lost in the merge. Seek times near the right-hand column mean the
last two were. Fix it before committing.

For the five effects, the same folder has `FX <effect> CAM.fcproj` and `FX <effect> ADJ.fcproj`
(the effect on the camera clip and on an adjustment layer). Run them when the merge touched
`gpufx.rs`, `gpufx_more.rs`, `fx.rs` or `fx.wgsl`, with the same `project` command. After our
work each showed 568–581 of 600; before, 0–4. `FX Twirl` is the control: it has no GPU version
and stays near 0.

A second project, `FULL TEST 02.fcproj` (sequence `EDIT_PR_282 News Nov 02`), exercises the
second round of optimizations. Run the same `project` command on it; frames shown of 600:

| Start | What it exercises | With our work | Without |
|---|---|---|---|
| 140 | adjustment layer nudged 10 px off the frame | 558 | 0 |
| 720 | the same with a Lens Distortion transition | 583 | 11 |
| 380 | Emboss and VR Digital Glitch transition | 569 | 544 |

`FX Color Grading CAM.fcproj` and `FX Color Grading ADJ.fcproj` (same sequence name, start 140)
carry a Color Grading with every section in use: 559 and 561 with our work. Without it the whole
effect renders on the CPU; a window that falls to a handful of frames means that was lost.

If upstream is now faster than us somewhere, that is not a reason to change anything; report it.

## 6. Commit

1. `git add -A` and check that `git status` shows no unmerged file and that
   `grep -rn "<<<<<<<\|>>>>>>>" crates apps` finds nothing.
2. Commit the merge with a message that names the release and says how each conflict was
   resolved. One commit: the merge and its fixes together. Do not push.
3. Confirm `target/release/filmcraft --version` prints the new version.
4. Add a line to the history below, in the same commit or a following one.

The report to Bruno comes after the installation (sections 7 and 8).

## 7. Install

Install only a committed merge that passed sections 4 and 5. The installation replaces the
`filmcraft` package on the system with one built from this checkout. It is an ordinary package
installation: Bruno is asked for his password.

**1. Build the install tree.** The project's own packaging script builds the two programs the
way official packages are built (locked dependencies, natural voices included) and lays out the
files as they go on the system:

```sh
packaging/linux/package.sh --formats tar
rm -rf dist/release
```

It leaves the tree in `target/linux-package/root/`. The `.tar.gz` it also writes into
`dist/release/` is not used, hence the `rm`. Do not ask for the `deb` format: it needs a tool
(`nfpm`) that is not installed here, and none is to be installed.

**2. Make the package** from that tree. Run this as one block, from the repository root:

```sh
set -e
V=$(awk -F'"' '/^\[workspace.package\]/{p=1} p&&/^version/{print $2; exit}' Cargo.toml)
PV="$V-1+fork.$(git rev-parse --short=8 HEAD)"
W=target/linux-package
rm -rf "$W/deb"; cp -R "$W/root" "$W/deb"; mkdir "$W/deb/DEBIAN"
cat > "$W/deb/DEBIAN/control" <<EOF
Package: filmcraft
Version: $PV
Architecture: amd64
Maintainer: Bruno (personal fork)
Section: video
Priority: optional
Installed-Size: $(du -sk "$W/deb/usr" | cut -f1)
Depends: libc6 (>= 2.35), libgcc-s1, libxkbcommon0, libvulkan1 | libegl1, libasound2 | libasound2t64
Description: FilmCraft video editor, personal fork with faster timeline playback.
 Built from this checkout; replaces the official filmcraft package.
EOF
install -m755 packaging/linux/postinst.sh "$W/deb/DEBIAN/postinst"
install -m755 packaging/linux/postinst.sh "$W/deb/DEBIAN/postrm"
mkdir -p dist/fork
dpkg-deb --build --root-owner-group "$W/deb" "dist/fork/filmcraft-$PV.deb"
rm -rf "$W/deb"
```

The package is named `filmcraft`, like the official one, so installing it replaces whatever
`filmcraft` package is there (the official one the first time, the previous fork build later).
Nothing has to be uninstalled first. `dist/` is ignored by Git.

**3. Install it.** FilmCraft must be closed (`pgrep -a -x filmcraft` prints nothing).

```sh
sudo dpkg -i "dist/fork/filmcraft-$PV.deb"
```

`sudo` asks for Bruno's password. If you cannot show him a password prompt, ask through your
environment's secure secret prompt if it has one; otherwise give him that one command, with the
file name written out, to run himself. Never ask for the password in the conversation, and never
store it.

**4. Check the installation.**

```sh
dpkg -s filmcraft | grep '^Version'      # ends in +fork.<the merge commit>
/usr/bin/filmcraft --version              # the new release number
/usr/bin/filmcraft-cli --version
sha256sum /usr/bin/filmcraft target/linux-package/root/usr/bin/filmcraft   # the same hash twice
```

**5. Keep one way back.** In `dist/fork/`, keep the package just installed and the one before
it; delete older ones. To go back: `sudo dpkg -i dist/fork/<previous>.deb`.

Things to leave alone:

- **Bruno's launcher.** His menu entry (`~/.local/share/applications/ai.storyteller.filmcraft.desktop`)
  and the `filmcraft` command (`~/.local/bin/filmcraft`) start `/usr/bin/filmcraft` through a
  script of his outside this repository. They keep working because the package installs to the
  same paths. Do not edit them.
- **His settings and presets** in `~/.local/share/filmcraft/`. The package does not touch them.
- **`.mcp.json` in this repository**, which points at `target/release/filmcraft-cli`.

## 8. Report

Report to Bruno: the release merged, the conflicts and how they were resolved, anything of
upstream's that was set aside in favour of ours, the checks that ran and the ones that did not,
the benchmark numbers next to the table in section 5, the installed package version, and the
sizes of `target/` and `dist/`.

Launch the app only if Bruno asks. He normally opens it from his menu.

To undo an update that has not been pushed: reinstall the previous package (section 7, step 5),
then `git reset --hard pre-<version>-merge`. The reset discards the merge; do it only when Bruno
asks.

## Update history

| Date | Release | Base before | Conflicts | Result | Installed |
|---|---|---|---|---|---|
| 2026-11-14 | v0.20.1 (`cedf0c9b`) | upstream `main` at `0dba3b1e` (2026-11-10, version 0.17.0) | `crates/codecs/src/gop.rs`, `crates/codecs/src/gop/tests.rs`, `crates/render/src/lib.rs`; `mask_scale` added in `gpufx.rs` and `gpufx_more.rs` | Merge `28061b8e`. Play 581 / 597 / 573, seek 291 / 441 ms. | `0.20.1-1+fork.28061b8e`, over the official `0.20.1-1`. Built without the extra fonts (`CRAFT_FONTS_DIR` unset), by Bruno's choice. |
