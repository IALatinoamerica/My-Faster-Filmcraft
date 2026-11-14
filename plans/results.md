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

## Effect test projects

`FX <effect> CAM.fcproj` has the effect on every camera clip, `FX <effect> ADJ.fcproj` on a
full-frame adjustment layer over the whole sequence (copies of `SHORT TEST.fcproj`). 10 s from
5 s at Full; frames shown of 600.

| Project | After 02 | After 06 |
|---|---|---|
| Luma Key CAM | 0 | 145 |
| Rounded Crop CAM | 4 | 247 |
| Noise CAM | 1 | 23 |
| Twirl CAM | 1 | 2 |
| Ultra Key CAM | 0 | 2 |
| Ultra Key Soft CAM | 0 | 0 |
| Drop Shadow CAM | 0 | 0 |
| every ADJ project | 0 | 0 |

Plan 06 removes the full-frame canvas and its resample, but the camera's working image is the
full 1920×1080 frame (it is shown at 51.5 %, above the half size at which a reduced frame is
decoded), so a CPU effect on it still costs a full-frame float image and its upload per frame.
It is restricted to media clips: a transformed nested sequence placed by the GPU did not match
the CPU picture (`export_renderer::tests::transformed_nested_picture_matches_cpu`).
