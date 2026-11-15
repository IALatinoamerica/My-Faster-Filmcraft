# Results, round 2

`bench_playback --hw auto --scenario project --project "FULL TEST 02.fcproj" --sequence
"EDIT_PR_282 News Nov 02" --start <s> --seconds 10 --res full`: frames shown of 600 due.
FilmCraft was open on the same machine during the baseline sweep.

| Window (start) | What is there | Before | After 01 |
|---|---|---|---|
| 122 s | zoom layer nudged 10 px | 3 | 570 |
| 140 s | same, whole window | 0 | 558 |
| 220 s | same, whole window | 0 | 556 |
| 1200 s | same, whole window | 0 | 572 |
| 280 s | zoom layer + Lens Distortion transition | 302 | 526 |
| 720 s | zoom layer + Lens Distortion transition | 11 | 547 |

CPU per due frame in the first four windows after 01: 9.1–9.4 ms.
