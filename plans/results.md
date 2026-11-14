# Results

`bench_playback --hw auto --scenario project|project-seek --project "FULL TEST.fcproj"
--sequence "EDIT_PR_277 News Nov 02" --start <s> --seconds 10 --res <full|half>`, 60 Hz refresh,
6 frame workers, RX 6700 XT, VA-API hardware decoding on (0 declined, 0 fallbacks in every run).
600 frames are due per play.

- **Play A**: start 0 s. Screen recording + masked camera, no adjustment layer.
- **Play B**: start 815 s. The same under a Transform zoom with its own shutter angle.
- **Play C**: start 60 s. The same under a Transform zoom without its own shutter angle.
- **Seek**: 40 random jumps over the whole sequence; time until the exact frame is shown.

| Step | Play A full shown | Play A half shown | Play B full shown | Play C full shown | CPU ms per frame (A full) | Seek p50 / p95 ms |
|---|---|---|---|---|---|---|
| Baseline | 1 | 543 | 0 | 1 | 159.3 | 606 / 907 |
| 01 masks on the GPU | 551 | 551 | 0 | 535 | 8.7 | 607 / 925 |
| 02 shutter Transform on the GPU | 554 | 543 | 598 | 537 | 8.4 | 524 / 841 |

Notes from the baseline:

- Hardware decoding is used for every picture, but it is the slow part that is left: 6 to 8 ms of
  wall time per picture inside the decoder (read-back included), against about 4 ms per picture
  for the software decoder on the same clips. Two 60 fps sources need 120 pictures a second.
- A seek decodes about 66 pictures per source; decoding is 500 of the 606 ms.
