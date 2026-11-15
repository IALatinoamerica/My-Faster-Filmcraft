# Stage 3: the report in the interface

## Purpose

Bruno uses voice input and should not have to run a command to learn that a clip will stutter.
The list from stage 2 is shown in the app.

## Change

**A window, opened from the menu.** `Sequence ▸ CPU Report…` runs the stage 2 command for the
active sequence and opens a window with:

- a first line: "Nothing in this sequence renders on the CPU." or "N clips render on the CPU; M
  of them take the whole frame.";
- one row per entry, whole-frame entries first and marked as such: track, clip name, start
  timecode, duration, reason;
- clicking a row moves the playhead to the entry's start and selects the clip;
- a Refresh button. The list is computed when the window opens and on Refresh, not on every
  frame.

The report uses the capabilities of the device the app is really rendering with (the stage 2
command reports `"device": "actual"` here).

**Rules of this codebase that apply** (AGENTS.md §3.2):

- every widget registers an automation id, and the window's state is serde so the control
  channel can read and set it;
- interface text goes through `tl!` / `tlf!` with Spanish rows in
  `crates/ui-egui/src/i18n/es.tsv`; the reason strings come from `crates/render` in English and
  are shown as they are (translating them would need a catalog row per reason and per effect
  name; not in this stage);
- no panics: an empty or failed report shows a message.

Find the closest existing window of the same kind (a list window opened from a menu) and build
this one the same way; do not invent a new window framework.

## For Bruno to decide before this stage starts

1. **Should the app also warn by itself?** Options: (a) only the menu window above; (b) also a
   small mark on the Program Monitor while the frame under the playhead renders on the CPU;
   (c) also a mark on the affected clips in the timeline. Recommended: (a) plus (b). (b) is
   cheap for whole frames (`plan::is_cpu_frame` already answers it), and it is seen exactly
   when it matters. (c) means recomputing the report on every edit and drawing on the timeline,
   which upstream changes often: more to maintain at each update.
2. **Menu place and name.** Proposed: `Sequence ▸ CPU Report…`.

## Checks

- `cargo test --release -p filmcraft-ui-egui --lib`: the window opens with a synthetic project,
  lists the entries from stage 2, and a click on a row moves the playhead; the i18n tests pass.
- In the running app (control channel + `ui.screenshot`, see `docs/agents.md`), with
  `FX Twirl ADJ.fcproj`: the window lists the Twirl layers, and a click jumps to one. Say in the
  report whether this was run.
- If option (b) is chosen: the mark appears on a Twirl frame and not on a clean frame.
- The benchmark on window 140 s of `FULL TEST 02.fcproj` is unchanged (558 of 600): the report
  must cost nothing during playback.
