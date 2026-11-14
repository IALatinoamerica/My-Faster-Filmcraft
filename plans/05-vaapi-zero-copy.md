# 05 — VA-API pictures without the CPU round trip (not recommended now)

## Problem

Each hardware-decoded picture is copied GPU → CPU (`vaGetImage`), deinterleaved from NV12 into
planar Y'CbCr (`crates/platform/src/biplanar.rs`), then uploaded CPU → GPU as three textures on the
UI thread (`GpuCompositor::upload`). The picture never needed to leave the GPU.

## Size of the cost here

A 1080p 8-bit picture is 3.1 MB. Two layers at 60 fps is about 370 MB/s each way. Upstream measured
2.3 ms of CPU per 1080p HEVC frame for the whole hardware path on a laptop. That is small next to
plans 01 and 02, and plan 03b already removes the read-back for the pictures a seek discards.

## Options

| Option | Work | Gain at 1080p |
|---|---|---|
| A. Upload NV12 as it is (R8 + RG8 textures, deinterleave in the shader) | Small: `biplanar.rs`, a new `PixelData` layout or flag, `composite.wgsl` | Saves one 1 MB pass per picture |
| B. `vaDeriveImage` instead of `vaGetImage` | Small, in `vaapi/va.rs` | Driver-dependent; on `radeonsi` the derived image is in write-combined memory and reading it can be slower |
| C. Zero-copy: `vaExportSurfaceHandle` (DMA-BUF) imported into Vulkan and wrapped as a wgpu texture | Large: `unsafe` Vulkan external-memory code through `wgpu-hal`, surface lifetime tied to the DPB, a second path for exports and thumbnails, Vulkan-only | Removes both copies |

## Recommendation

Do not do this now. Option C is the only one with a real gain, and it is the largest and least
mergeable change of all the plans: it needs `unsafe` outside the rules `crates/platform` allows
today (a wgpu texture made from a foreign handle), and upstream lists zero-copy as its own roadmap
item, so a fork-side version would conflict when theirs lands.

Revisit if 4K sources become the normal workload (12.4 MB per picture) or if plan 00 shows
read-back and upload as the largest remaining cost after plans 01–03.
