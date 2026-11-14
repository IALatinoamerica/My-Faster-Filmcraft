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
| 03a sources fetched side by side | 549 | 555 | 598 | 530 | 9.3 | 372 / 556 |
| 03b no read-back for pictures nobody will show | 583 | 583 | 598 | 569 | 8.4 | 272 / 405 |

Notes from the baseline:

- Hardware decoding is used for every picture, but it is the slow part that is left: 6 to 8 ms of
  wall time per picture inside the decoder (read-back included), against about 4 ms per picture
  for the software decoder on the same clips. Two 60 fps sources need 120 pictures a second.
- A seek decodes about 66 pictures per source; decoding is 500 of the 606 ms.

## Effect test projects

`FX <effect> CAM.fcproj` has the effect on every camera clip, `FX <effect> ADJ.fcproj` on a
full-frame adjustment layer over the whole sequence (copies of `SHORT TEST.fcproj`). 10 s from
5 s at Full; frames shown of 600.

| Project | After 02 | After 06 | After 07 |
|---|---|---|---|
| Luma Key CAM / ADJ | 0 / 0 | 145 / 0 | 516 / 491 |
| Rounded Crop CAM / ADJ | 4 / 0 | 247 / 0 | 518 / 511 |
| Noise CAM / ADJ | 1 / 0 | 23 / 0 | 495 / 490 |
| Ultra Key CAM / ADJ | 0 / 0 | 2 / 0 | 517 / 494 |
| Ultra Key Soft CAM / ADJ (choke and soften) | 0 / 0 | 0 / 0 | 511 / 517 |
| Drop Shadow CAM / ADJ | 0 / 0 | 0 / 0 | 503 / 510 |
| Twirl CAM / ADJ (no GPU version) | 1 / 0 | 2 / 0 | 3 / 0 |

After 07 the process uses about 8 ms of CPU per frame in every project but Twirl (190 to 240 ms).
After 03a and 03b the same projects show 568 to 581 of 600 (Twirl: 6 and 0).
The frames still dropped are the ones the test projects without effects drop too: decoding.

Plan 06 removes the full-frame canvas and its resample, but the camera's working image is the
full 1920×1080 frame (it is shown at 51.5 %, above the half size at which a reduced frame is
decoded), so a CPU effect on it still costs a full-frame float image and its upload per frame.
It is restricted to media clips: a transformed nested sequence placed by the GPU did not match
the CPU picture (`export_renderer::tests::transformed_nested_picture_matches_cpu`).

## VA-API time per picture on this GPU (measured with temporary counters, before 03b)

| Step | ms per picture |
|---|---|
| Submitting the picture | 0.1 to 0.6 |
| Waiting for the GPU to decode it (`vaSyncSurface`) | 1.5 to 2.4 |
| Copying it out of video memory (`vaGetImage`) | 1.2 to 2.2 |
| Mapping the copy and splitting the chroma plane | 0.6 to 1.8 |

Read-back is about half of what a picture costs. 03b skips it for the pictures a seek decodes
only as references: all but the 24 samples before the wanted one. The frames still dropped in
Play A and Play C (17 and 31 of 600) fall at cuts, where a decoder starts from a keyframe; that is
plan 03c.
