# 03 — Emboss and VR Digital Glitch on the GPU

**Emboss** (`effects.rs`): two edge-clamped bilinear reads at ±(direction × relief), their luma
difference as a grey level, mixed with the picture by Blend.

**VR Digital Glitch** (`vfx/immersive.rs`): rows grouped in bands; a hash of (band, time step,
seed) decides which bands shift sideways (wrapping), split red and blue, and odd rows darken as
scanlines. Only the monoscopic layout goes to the GPU; stereo layouts stay on the CPU.

Both: a struct in `gpufx_more.rs` with `eval` / `gpu_ok` / `apply`, an entry in `GPU_EFFECTS`,
one pass in `fx.rs` / `fx.wgsl`. Tests as in plan 02.
