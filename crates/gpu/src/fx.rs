//! The GPU effect stage: standard effects of a plan layer ([`LayerFx`]) run in WGSL (`fx.wgsl`).
//!
//! A layer with effects is first drawn from its source (YUV planes or RGBA, through the
//! compositor's own `fs` shader, so decoding is the same as for a plain layer) into a working
//! texture of the size the CPU decodes it at; each [`FxOp`] then runs as compute passes that
//! ping-pong between two working textures (three for Unsharp Mask, which keeps the original);
//! the last one is drawn into the accumulator with the layer's matrix, opacity and blend mode
//! like any RGBA layer. Working textures are `Rgba32Float` (the CPU reference's precision) and
//! pooled per size; layers without effects never touch any of this.
//!
//! Box blurs (Gaussian Blur, Camera Blur, the blur inside Unsharp / Sharpen) run per pixel for
//! radii up to [`BOX_PER_PIXEL_MAX`] and as a running sum per row / column above, so a huge
//! radius costs O(1) per pixel like on the CPU (radii are capped by `gaussian_boxes`).

use std::collections::{HashMap, HashSet};

use filmcraft_render::gpufx::FxOp;
use filmcraft_render::plan::LayerFx;

/// Working texture format.
pub(crate) const FX_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba32Float;
/// Box radii up to this many pixels are summed per output pixel; larger ones as a running sum.
pub const BOX_PER_PIXEL_MAX: u32 = 32;

const OP_BRIGHTNESS_CONTRAST: u32 = 1;
const OP_PROC_AMP: u32 = 2;
const OP_TINT: u32 = 3;
const OP_BLACK_WHITE: u32 = 4;
const OP_COLOR_BALANCE: u32 = 5;
const OP_LEAVE_COLOR: u32 = 6;
const OP_CHANGE_TO_COLOR: u32 = 7;
const OP_COLOR_PASS: u32 = 8;
const OP_GAMMA: u32 = 9;
const OP_LEVELS: u32 = 10;
const OP_EXTRACT: u32 = 11;
const OP_INVERT: u32 = 12;
const OP_INVERT_ALPHA: u32 = 13;
const OP_POSTERIZE: u32 = 14;
const OP_ASC_CDL: u32 = 15;
const OP_CHANNEL_MIX: u32 = 16;
const OP_COLOR_REPLACE: u32 = 17;
const OP_ALPHA_ADJUST: u32 = 18;
const OP_VIGNETTE: u32 = 19;
const OP_BOX: u32 = 20;
const OP_DIRECTIONAL: u32 = 22;
const OP_UNSHARP: u32 = 23;
const OP_CROP: u32 = 24;
const OP_RESAMPLE: u32 = 25;
const OP_HFLIP: u32 = 26;
const OP_VFLIP: u32 = 27;
const OP_MIRROR: u32 = 28;
const OP_OFFSET: u32 = 29;
const OP_VIDEO_LIMITER: u32 = 30;
const OP_LUMETRI: u32 = 31;
const OP_BOKEH_DOWN: u32 = 32;
const OP_BOKEH_BOOST: u32 = 33;
const OP_BOKEH: u32 = 34;
const OP_BOKEH_UP: u32 = 35;
const OP_THREE_WAY: u32 = 36;
const OP_MASK: u32 = 100;
const OP_RESAMPLE_ACC: u32 = 101;
const OP_LUMA_KEY: u32 = 102;
const OP_ROUNDED_CROP: u32 = 103;
const OP_SHADOW: u32 = 104;
const OP_OVER_ORIG: u32 = 105;
const OP_NOISE: u32 = 106;
const OP_KEY_MATTE: u32 = 107;
const OP_KEY_CHOKE: u32 = 108;
const OP_KEY_FINAL: u32 = 109;
const OP_MIX_BASE: u32 = 110;
const OP_LENS: u32 = 111;
const OP_EMBOSS: u32 = 112;
const OP_GLITCH: u32 = 113;
const OP_GRADE_ADV: u32 = 114;
const OP_LUT: u32 = 115;
const OP_HSL_KEY: u32 = 116;
const OP_HSL_MEDIAN: u32 = 117;
const OP_HSL_FINAL: u32 = 118;
/// `effects::hash3`'s multiplier of the seed (the shader gets the product).
const NOISE_SEED_MUL: u64 = 0x1656_67B1_9E37_79F9;

/// Coverage planes of opacity masks kept as textures (a static mask is one plane for every frame).
const MASK_PLANES: usize = 8;
/// A table of more values than this is kept as a texture like a plane (a LUT); a smaller one
/// (curves, the rows of a glitch) is made for the job.
const KEPT_TABLE: usize = 1 << 14;

type Target = (wgpu::Texture, wgpu::TextureView);

pub(crate) struct FxStage {
    px: wgpu::ComputePipeline,
    run: wgpu::ComputePipeline,
    bgl: wgpu::BindGroupLayout,
    /// Draws a layer's source into a working texture (`composite.wgsl` `fs`, no blending).
    source: wgpu::RenderPipeline,
    pool: HashMap<(u32, u32), Vec<Target>>,
    used: HashSet<(u32, u32)>,
    /// Open [`begin_frame`](Self::begin_frame)s.
    frames: u32,
    /// Working textures created so far (a steady stream of frames creates none).
    created: u64,
    /// Uploaded coverage planes of [`FxOp::Mask`], most recently used last. The plane is kept so
    /// its address stays its identity.
    planes: Vec<(std::sync::Arc<Vec<f32>>, wgpu::TextureView)>,
}

/// One compute pass of a job.
struct Dispatch {
    run: bool,
    bg: wgpu::BindGroup,
    groups: (u32, u32),
}

/// The recorded work of one layer's effects (bind groups made; recorded into the frame's encoder
/// right before the layer is drawn, so layers can share pooled textures).
pub(crate) struct FxJob {
    source_bg: wgpu::BindGroup,
    source_target: wgpu::TextureView,
    dispatches: Vec<Dispatch>,
    /// The texture holding the result.
    pub(crate) result: wgpu::TextureView,
    pub(crate) result_texture: wgpu::Texture,
}

/// One pass before texture routing: entry point, op code, integer and float parameters.
struct Step {
    run: bool,
    code: u32,
    i1: [u32; 4],
    p: [f32; 24],
    /// Unsharp combine: reads the blurred image and the original.
    combine: bool,
    /// A blur pass of Unsharp Mask (the original stays untouched until the combine).
    unsharp_blur: bool,
    /// Bokeh Blur's gather: its kernel (`filmcraft_render::vfx::bokeh_kernel`), read as the aux
    /// texture.
    kernel: Vec<(i32, i32, f32)>,
    /// An opacity mask's coverage plane (one value per pixel), read as the aux texture.
    plane: Option<std::sync::Arc<Vec<f32>>>,
    /// A pass of a sum over several placements of the original (Transform's shutter blur): reads
    /// the sum so far and, as the aux texture, the original, which stays untouched until the last
    /// pass (`combine`).
    reads_orig: bool,
    /// The mix of an adjustment layer with the picture below it: reads, as the aux texture, the
    /// picture the job started from (its first texture, kept untouched while such a job runs).
    mix_base: bool,
    /// A table of numbers read through the table binding: (values, width, height), one `R32Float`
    /// texel each, row by row (a coverage plane, curves, a LUT).
    table: Option<(std::sync::Arc<Vec<f32>>, u32, u32)>,
}

fn step(code: u32, i1: [u32; 4], p: &[f32]) -> Step {
    let mut q = [0.0; 24];
    for (d, s) in q.iter_mut().zip(p) {
        *d = *s;
    }
    Step { run: false, code, i1, p: q, combine: false, unsharp_blur: false, kernel: Vec::new(), plane: None, reads_orig: false, mix_base: false, table: None }
}

/// The pass of a Color Grading LUT: the LUT on the display-encoded colour, mixed in by `mix`;
/// `decode` writes the linear colour, otherwise the encoded signal goes on to the next pass.
fn lut_step(l: &filmcraft_render::gpufx_more::GradeLut, mix: f32, decode: bool) -> Step {
    use filmcraft_render::gpufx_more::TABLE_W;
    let (n1, size) = l.sizes();
    let dom =
        |d: Option<(&[f32; 3], &[f32; 3])>| d.map_or([0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 0.0], |(lo, hi)| [lo[0], lo[1], lo[2], 0.0, hi[0], hi[1], hi[2], 0.0]);
    let shaper = dom(l.lut.shaper.as_ref().map(|s| (&s.domain_min, &s.domain_max)));
    let cube = dom(l.lut.cube.as_ref().map(|c| (&c.domain_min, &c.domain_max)));
    let mut p = vec![mix, 0.0, 0.0, 0.0];
    p.extend(shaper);
    p.extend(cube);
    let mut s = step(OP_LUT, [n1 as u32, size as u32, decode as u32, 0], &p);
    s.table = Some((l.table.clone(), TABLE_W as u32, (l.table.len() / TABLE_W) as u32));
    s
}

/// `values` (`w` × `h` of them, row by row) as a texture of one `R32Float` texel each.
fn table_texture(device: &wgpu::Device, queue: &wgpu::Queue, values: &[f32], w: u32, h: u32) -> wgpu::TextureView {
    let size = wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 };
    let t = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("filmcraft-fx-table"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R32Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
    queue.write_texture(
        t.as_image_copy(),
        &bytes,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w.saturating_mul(4)), rows_per_image: Some(h) },
        size,
    );
    t.create_view(&Default::default())
}

/// Texels a row of a kernel texture holds.
const KERNEL_ROW: usize = 64;

/// Bokeh Blur's kernel as a texture: entry `k` at (`k` % [`KERNEL_ROW`], `k` / [`KERNEL_ROW`]),
/// (dx, dy, weight, 0). At most 2500 entries: 40 rows.
fn kernel_texture(device: &wgpu::Device, queue: &wgpu::Queue, kernel: &[(i32, i32, f32)]) -> wgpu::TextureView {
    let rows = kernel.len().div_ceil(KERNEL_ROW).max(1);
    let mut texels = vec![0f32; rows * KERNEL_ROW * 4];
    for (t, &(dx, dy, wt)) in texels.as_chunks_mut::<4>().0.iter_mut().zip(kernel) {
        *t = [dx as f32, dy as f32, wt, 0.0];
    }
    let size = wgpu::Extent3d { width: KERNEL_ROW as u32, height: rows as u32, depth_or_array_layers: 1 };
    let t = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("filmcraft-fx-kernel"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba32Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let bytes: Vec<u8> = texels.iter().flat_map(|v| v.to_le_bytes()).collect();
    queue.write_texture(
        t.as_image_copy(),
        &bytes,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some((KERNEL_ROW * 16) as u32), rows_per_image: Some(rows as u32) },
        size,
    );
    t.create_view(&Default::default())
}

/// Box passes along x (vertical = false) or y for the radii (radius 0 is a no-op on the CPU).
fn boxes(radii: &[u32], vertical: bool, repeat: bool, out: &mut Vec<Step>) {
    for &r in radii.iter().filter(|r| **r > 0) {
        let mut s = step(OP_BOX, [r, repeat as u32, vertical as u32, 0], &[]);
        s.run = r > BOX_PER_PIXEL_MAX;
        out.push(s);
    }
}

/// The passes of an op on a `w`×`h` working image (empty: no-op). Unsharp's blur passes come
/// first, then the combine.
fn steps(op: &FxOp, w: u32, h: u32) -> Vec<Step> {
    let mut out = Vec::new();
    match op {
        FxOp::BrightnessContrast { br, co } => out.push(step(OP_BRIGHTNESS_CONTRAST, [0; 4], &[*br, *co])),
        FxOp::ProcAmp { br, co, hue, sat } => out.push(step(OP_PROC_AMP, [0; 4], &[*br, *co, *hue, *sat])),
        FxOp::Tint { black, white, amount } => {
            out.push(step(OP_TINT, [0; 4], &[black[0], black[1], black[2], *amount, white[0], white[1], white[2]]));
        }
        FxOp::BlackWhite => out.push(step(OP_BLACK_WHITE, [0; 4], &[])),
        FxOp::ColorBalance { sh, md, hi, preserve } => {
            out.push(step(OP_COLOR_BALANCE, [*preserve as u32, 0, 0, 0], &[sh[0], sh[1], sh[2], 0.0, md[0], md[1], md[2], 0.0, hi[0], hi[1], hi[2]]))
        }
        FxOp::ThreeWay { sh, md, hi, range, sat } => out.push(step(
            OP_THREE_WAY,
            [op.is_identity_three_way() as u32, 0, 0, 0],
            &[sh[0], sh[1], sh[2], sh[3], md[0], md[1], md[2], md[3], hi[0], hi[1], hi[2], hi[3], range[0], range[1], range[2], range[3], *sat],
        )),
        FxOp::LeaveColor { amount, key_hue, tol, soft } => out.push(step(OP_LEAVE_COLOR, [0; 4], &[*amount, *key_hue, *tol, *soft])),
        FxOp::ChangeToColor { from_hue, to_hue, tol, soft } => out.push(step(OP_CHANGE_TO_COLOR, [0; 4], &[*from_hue, *to_hue, *tol, *soft])),
        FxOp::ColorPass { key, sim, reverse } => out.push(step(OP_COLOR_PASS, [*reverse as u32, 0, 0, 0], &[key[0], key[1], key[2], *sim])),
        FxOp::Gamma { g } => out.push(step(OP_GAMMA, [0; 4], &[*g])),
        FxOp::Levels { ib, iw, ob, ow, g } => out.push(step(OP_LEVELS, [0; 4], &[*ib, *iw, *ob, *ow, *g])),
        FxOp::Extract { lo, hi, soft, invert } => out.push(step(OP_EXTRACT, [*invert as u32, 0, 0, 0], &[*lo, *hi, *soft])),
        FxOp::Invert { channel: 4, blend } => out.push(step(OP_INVERT_ALPHA, [0; 4], &[*blend])),
        FxOp::Invert { channel, blend } => out.push(step(OP_INVERT, [*channel, 0, 0, 0], &[*blend])),
        FxOp::Posterize { n } => out.push(step(OP_POSTERIZE, [0; 4], &[*n])),
        FxOp::Gaussian { rx, ry, repeat } => {
            boxes(rx, false, *repeat, &mut out);
            boxes(ry, true, *repeat, &mut out);
        }
        FxOp::DirectionalBlur { dx, dy, steps } => {
            if *steps >= 2 {
                out.push(step(OP_DIRECTIONAL, [*steps, 0, 0, 0], &[*dx, *dy]));
            }
        }
        FxOp::Bokeh { levels, boost, threshold, sides, rot, rr, rings } => {
            if *rings > 0 {
                // `vfx::bokeh` on the top-left part of the working textures: each halving writes
                // the half-size image there, the boost and the gather work in place, and the
                // last pass scales it back up to the whole image.
                let (mut ww, mut wh) = (w, h);
                for _ in 0..*levels {
                    out.push(step(OP_BOKEH_DOWN, [ww, wh, 0, 0], &[]));
                    (ww, wh) = ((ww / 2).max(1), (wh / 2).max(1));
                }
                if *boost > 0.0 {
                    out.push(step(OP_BOKEH_BOOST, [ww, wh, 0, 0], &[*boost, *threshold]));
                }
                let kernel = filmcraft_render::vfx::bokeh_kernel(*sides, *rot, *rr, *rings);
                let mut gather = step(OP_BOKEH, [ww, wh, kernel.len() as u32, 0], &[]);
                gather.kernel = kernel;
                out.push(gather);
                if *levels > 0 {
                    // `Image::transformed` by scale(w / ww, h / wh): its inverse, in f64 as there
                    let m = filmcraft_geom::Affine::scale(w as f64 / ww as f64, h as f64 / wh as f64);
                    if let Some(inv) = m.inverse() {
                        out.push(step(OP_BOKEH_UP, [ww, wh, 0, 0], &[inv.a as f32, inv.d as f32, inv.e as f32, inv.f as f32]));
                    }
                }
            }
        }
        FxOp::Unsharp { rx, ry, amount, threshold } => {
            boxes(rx, false, true, &mut out);
            boxes(ry, true, true, &mut out);
            for s in &mut out {
                s.unsharp_blur = true;
            }
            let mut s = step(OP_UNSHARP, [0; 4], &[*amount, *threshold]);
            s.combine = true;
            out.push(s);
        }
        FxOp::Crop { x0, x1, y0, y1, feather } => out.push(step(OP_CROP, [0; 4], &[*x0, *x1, *y0, *y1, *feather])),
        FxOp::Resample(r) => {
            let op = if (r.opacity - 1.0).abs() < 1e-6 { 1.0 } else { r.opacity };
            match &r.inv {
                Some(m) => out.push(step(OP_RESAMPLE, r.rect, &[m.a as f32, m.b as f32, m.c as f32, m.d as f32, m.e as f32, m.f as f32, op, 1.0])),
                None => out.push(step(OP_RESAMPLE, [0; 4], &[0.0; 8])),
            }
        }
        FxOp::HFlip => out.push(step(OP_HFLIP, [0; 4], &[])),
        FxOp::VFlip => out.push(step(OP_VFLIP, [0; 4], &[])),
        FxOp::Mirror { cx, cy, nx, ny } => out.push(step(OP_MIRROR, [0; 4], &[*cx, *cy, *nx, *ny])),
        FxOp::Offset { dx, dy, blend } => out.push(step(OP_OFFSET, [0; 4], &[*dx, *dy, *blend])),
        FxOp::AscCdl { slope: s, offset: o, power: p, sat } => {
            out.push(step(OP_ASC_CDL, [0; 4], &[s[0], s[1], s[2], *sat, o[0], o[1], o[2], 0.0, p[0], p[1], p[2]]));
        }
        FxOp::ChannelMix { m } => {
            let p: Vec<f32> = m.iter().flatten().copied().collect();
            out.push(step(OP_CHANNEL_MIX, [0; 4], &p));
        }
        FxOp::ColorReplace { sim, solid, target: t, replace: r, replace_hsl: h } => {
            out.push(step(OP_COLOR_REPLACE, [*solid as u32, 0, 0, 0], &[t[0], t[1], t[2], *sim, r[0], r[1], r[2], 0.0, h[0], h[1], h[2]]))
        }
        FxOp::AlphaAdjust { opacity, ignore, invert, mask_only } => {
            out.push(step(OP_ALPHA_ADJUST, [*ignore as u32, *invert as u32, *mask_only as u32, 0], &[*opacity]));
        }
        FxOp::Vignette { amount, midpoint, roundness, feather, target } => {
            if amount.abs() >= 1e-5 {
                out.push(step(OP_VIGNETTE, [0; 4], &[*amount, *midpoint, *roundness, *feather, target[0], target[1], target[2]]));
            }
        }
        FxOp::VideoLimiter { max, comp, axis, warn, warning_color } => {
            out.push(step(OP_VIDEO_LIMITER, [*axis, *warn as u32, 0, 0], &[*max, *comp, warning_color[0], warning_color[1], warning_color[2]]));
        }
        FxOp::Lumetri {
            gains,
            exposure,
            contrast,
            hl,
            sh,
            wh,
            bl,
            sat,
            creative_on,
            faded,
            vib,
            st,
            ht,
            vignette_on,
            va,
            vmid,
            vround,
            vfeather,
            aspect,
            ..
        } => {
            let ge = [gains[0] * exposure, gains[1] * exposure, gains[2] * exposure];
            let b0 = -bl * 0.15;
            let w0 = 1.0 - wh * 0.15;
            let sh_k = sh * 0.35;
            let hl_k = hl * 0.35;
            let vround_aspect = filmcraft_render::gpufx::lumetri_vignette_aspect(*aspect, *vround);
            let st_k = [(st[0] - 0.5) * 0.3, (st[1] - 0.5) * 0.3, (st[2] - 0.5) * 0.3];
            let ht_k = [(ht[0] - 0.5) * 0.3, (ht[1] - 0.5) * 0.3, (ht[2] - 0.5) * 0.3];

            let p = [
                ge[0],
                ge[1],
                ge[2],
                b0,
                w0,
                sh_k,
                hl_k,
                *contrast,
                *faded,
                *sat,
                *vib,
                *va,
                *vmid,
                *vfeather,
                vround_aspect,
                0.0,
                st_k[0],
                st_k[1],
                st_k[2],
                0.0,
                ht_k[0],
                ht_k[1],
                ht_k[2],
                0.0,
            ];
            out.push(step(OP_LUMETRI, [*creative_on as u32, *vignette_on as u32, 0, 0], &p));
        }
        FxOp::ResampleBlur { samples } => {
            let k = 1.0 / samples.len().max(1) as f32;
            for (i, r) in samples.iter().enumerate() {
                let first = if i == 0 { 1.0 } else { 0.0 };
                let op = if (r.opacity - 1.0).abs() < 1e-6 { 1.0 } else { r.opacity };
                let mut s = match &r.inv {
                    Some(m) => step(OP_RESAMPLE_ACC, r.rect, &[m.a as f32, m.b as f32, m.c as f32, m.d as f32, m.e as f32, m.f as f32, op, 1.0, first, k]),
                    None => step(OP_RESAMPLE_ACC, [0; 4], &[0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, first, k]),
                };
                s.reads_orig = true;
                s.combine = i + 1 == samples.len();
                out.push(s);
            }
        }
        FxOp::LumaKey(k) => out.push(step(OP_LUMA_KEY, [k.invert as u32, 0, 0, 0], &[k.th, k.cut])),
        FxOp::RoundedCrop(c) => {
            out.push(step(OP_ROUNDED_CROP, [0; 4], &[c.x0, c.y0, c.x1, c.y1, c.rad, c.feather, c.border, 0.0, c.bc[0], c.bc[1], c.bc[2]]));
        }
        FxOp::DropShadow(d) => {
            // the shadow from the picture's alpha, its blur, then the untouched picture over it
            let c = d.color;
            let mut shadow = match &d.shift.inv {
                Some(m) => {
                    step(OP_SHADOW, d.shift.rect, &[m.a as f32, m.b as f32, m.c as f32, m.d as f32, m.e as f32, m.f as f32, d.opacity, 1.0, c[0], c[1], c[2]])
                }
                None => step(OP_SHADOW, [0; 4], &[0.0; 8]),
            };
            shadow.reads_orig = !d.only;
            out.push(shadow);
            let first = out.len();
            boxes(&d.rx, false, false, &mut out);
            boxes(&d.ry, true, false, &mut out);
            if !d.only {
                for s in out.iter_mut().skip(first) {
                    s.unsharp_blur = true;
                }
                let mut over = step(OP_OVER_ORIG, [0; 4], &[]);
                over.combine = true;
                out.push(over);
            }
        }
        FxOp::Noise(n) => {
            if n.is_active() {
                let zc = n.seed.wrapping_mul(NOISE_SEED_MUL);
                out.push(step(OP_NOISE, [zc as u32, (zc >> 32) as u32, n.colored as u32, n.clip as u32], &[n.amount, n.grain]));
            }
        }
        FxOp::UltraKey(k) => {
            // The matte is a function of the picture alone, so the picture stays untouched in one
            // texture while the matte is made, blurred and cleaned up in the others.
            let cc = k.cc.unwrap_or([1.0, 0.0, 1.0]);
            let p = [
                k.key[0],
                k.key[1],
                k.key[2],
                k.kmag2,
                k.tol,
                k.gain,
                k.highlight,
                k.shadow,
                k.pedestal,
                k.choke.as_ref().map_or(0.0, |c| c.2),
                k.contrast,
                k.mid,
                k.spill,
                k.desat,
                k.range,
                k.spill_luma,
                cc[0],
                cc[1],
                cc[2],
                if k.cc.is_some() { 1.0 } else { 0.0 },
            ];
            let i1 = [k.dom.min(2), k.output, 0, 0];
            let pass = |code: u32, out: &mut Vec<Step>| {
                let mut s = step(code, i1, &p);
                s.reads_orig = true;
                out.push(s);
            };
            let blur = |rx: &[u32], ry: &[u32], out: &mut Vec<Step>| {
                let first = out.len();
                boxes(rx, false, false, out);
                boxes(ry, true, false, out);
                for s in out.iter_mut().skip(first) {
                    s.unsharp_blur = true;
                }
            };
            pass(OP_KEY_MATTE, &mut out);
            if let Some((rx, ry, _)) = &k.choke {
                blur(rx, ry, &mut out);
                pass(OP_KEY_CHOKE, &mut out);
            }
            blur(&k.soften.0, &k.soften.1, &mut out);
            pass(OP_KEY_FINAL, &mut out);
            if let Some(last) = out.last_mut() {
                last.combine = true;
            }
        }
        FxOp::LensDistortion(l) => out.push(step(OP_LENS, [0; 4], &[l.k as f32, l.cx as f32, l.cy as f32, l.norm as f32])),
        FxOp::Seq(ops) => out.extend(ops.iter().flat_map(|op| steps(op, w, h))),
        FxOp::GradeAdvanced(g) => {
            use filmcraft_render::gpufx_more::{CURVE_IDS, CURVE_N};
            let wh = g.wheels.unwrap_or_default();
            let p = [
                g.look_k, 0.0, 0.0, 0.0, //
                wh[0][0], wh[0][1], wh[0][2], wh[0][3], //
                wh[1][0], wh[1][1], wh[1][2], wh[1][3], //
                wh[2][0], wh[2][1], wh[2][2], wh[2][3],
            ];
            // The Look LUT is a pass of its own (its table is the LUT). It hands the next pass the
            // display-encoded signal as it is, not clamped, as the CPU goes on with it.
            let rest = g.look > 0 || g.curves != 0 || g.wheels.is_some();
            if let Some(l) = &g.look_lut {
                out.push(lut_step(l, g.look_k, !rest));
            }
            if rest {
                let mut s = step(OP_GRADE_ADV, [g.look, g.curves, g.wheels.is_some() as u32, g.look_lut.is_some() as u32], &p);
                s.table = Some((g.table.clone(), CURVE_N as u32, CURVE_IDS.len() as u32));
                out.push(s);
            }
        }
        FxOp::GradeLut(l) => out.push(lut_step(l, 1.0, true)),
        FxOp::GradeHsl(k) => {
            // The key is a function of the picture alone: the picture stays untouched in one
            // texture while the key is made, cleaned and blurred in the others (as Ultra Key's
            // matte).
            let p = [k.hc, k.hr, k.smin, k.lmin, k.lmax, k.soft, k.htemp, k.htint, k.hsat, k.hshift, k.denoise.min(1.0)];
            let i1 = [k.show_mask, k.median as u32, 0, 0];
            let mut key = step(OP_HSL_KEY, i1, &p);
            key.reads_orig = true;
            out.push(key);
            let first = out.len();
            if k.median > 0 {
                out.push(step(OP_HSL_MEDIAN, i1, &p));
            }
            boxes(&k.blur, false, true, &mut out);
            boxes(&k.blur, true, true, &mut out);
            for s in out.iter_mut().skip(first) {
                s.unsharp_blur = true;
            }
            let mut last = step(OP_HSL_FINAL, i1, &p);
            last.reads_orig = true;
            last.combine = true;
            out.push(last);
        }
        FxOp::Emboss(m) => out.push(step(OP_EMBOSS, [0; 4], &[m.dx, m.dy, m.contrast, m.blend])),
        FxOp::DigitalGlitch(g) => {
            // (no rows: the effect does nothing)
            if g.rows.len() == h as usize * 2 && h > 0 {
                let mut s = step(OP_GLITCH, [0; 4], &[g.cs, g.scan]);
                s.table = Some((g.rows.clone(), 2, h));
                out.push(s);
            }
        }
        FxOp::MixBase { weight, cov } => {
            // a plane of another size is a no-op, as on the CPU
            let fits = cov.as_ref().is_none_or(|c| (w as usize).checked_mul(h as usize) == Some(c.len()));
            if fits {
                let mut s = step(OP_MIX_BASE, [cov.is_some() as u32, 0, 0, 0], &[*weight]);
                s.mix_base = true;
                s.table = cov.as_ref().map(|c| (c.clone(), w, h));
                out.push(s);
            }
        }
        FxOp::Mask { cov } => {
            // a plane of another size is a no-op, as on the CPU
            if (w as usize).checked_mul(h as usize) == Some(cov.len()) {
                let mut s = step(OP_MASK, [0; 4], &[]);
                s.plane = Some(cov.clone());
                out.push(s);
            }
        }
    }
    out
}

impl FxStage {
    /// Whether `device` can run the stage: compute shaders with 16×16 workgroups and a storage
    /// texture (WebGL2-class devices can't; they render effects on the CPU).
    pub(crate) fn supported(device: &wgpu::Device) -> bool {
        let l = device.limits();
        l.max_storage_textures_per_shader_stage >= 1
            && l.max_compute_invocations_per_workgroup >= 256
            && l.max_compute_workgroup_size_x >= 64
            && l.max_compute_workgroup_size_y >= 16
            && l.max_compute_workgroups_per_dimension >= 1
    }

    pub(crate) fn new(device: &wgpu::Device, composite: &wgpu::ShaderModule, layer_bgl: &wgpu::BindGroupLayout) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("filmcraft-fx"),
            source: wgpu::ShaderSource::Wgsl(include_str!("fx.wgsl").into()),
        });
        let tex = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fx"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                tex(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: FX_FORMAT,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
                tex(3),
                tex(4),
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("fx"), bind_group_layouts: &[Some(&bgl)], immediate_size: 0 });
        let compute = |entry: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("fx"),
                layout: Some(&pl),
                module: &shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let spl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("fx-source"),
            bind_group_layouts: &[Some(layer_bgl)],
            immediate_size: 0,
        });
        let source = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("fx-source"),
            layout: Some(&spl),
            vertex: wgpu::VertexState { module: composite, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: composite,
                entry_point: Some("fs_fx_source"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: FX_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        });
        Self {
            px: compute("fx_px"),
            run: compute("fx_run"),
            bgl,
            source,
            pool: HashMap::new(),
            used: HashSet::new(),
            frames: 0,
            created: 0,
            planes: Vec::new(),
        }
    }

    /// Start a frame: pooled textures of sizes no layer uses by [`end_frame`](Self::end_frame)
    /// are released then. Frames nest (a composite of several steps is one frame around the
    /// layers of each step): the pool is trimmed when the outermost one ends.
    pub(crate) fn begin_frame(&mut self) {
        if self.frames == 0 {
            self.used.clear();
        }
        self.frames = self.frames.saturating_add(1);
    }

    pub(crate) fn end_frame(&mut self) {
        self.frames = self.frames.saturating_sub(1);
        if self.frames == 0 {
            let used = &self.used;
            self.pool.retain(|k, _| used.contains(k));
        }
    }

    #[cfg(test)]
    pub(crate) fn created(&self) -> u64 {
        self.created
    }

    /// Coverage planes held as textures.
    #[cfg(test)]
    pub(crate) fn mask_planes(&self) -> usize {
        self.planes.len()
    }

    /// No working textures held.
    #[cfg(test)]
    pub(crate) fn is_idle(&self) -> bool {
        self.pool.is_empty()
    }

    fn targets(&mut self, device: &wgpu::Device, size: (u32, u32), n: usize) -> Vec<Target> {
        self.used.insert(size);
        let v = self.pool.entry(size).or_default();
        while v.len() < n {
            let t = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("filmcraft-fx"),
                size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: FX_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::STORAGE_BINDING
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = t.create_view(&Default::default());
            v.push((t, view));
            self.created = self.created.saturating_add(1);
        }
        v.iter().take(n).cloned().collect()
    }

    /// The texture of a mask's coverage plane (`w`×`h`, one `R32Float` texel per pixel), uploaded
    /// once and found again by the plane's address.
    fn plane_texture(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, cov: &std::sync::Arc<Vec<f32>>, w: u32, h: u32) -> wgpu::TextureView {
        if let Some(i) = self.planes.iter().position(|(c, _)| std::sync::Arc::ptr_eq(c, cov)) {
            let hit = self.planes.remove(i);
            let view = hit.1.clone();
            self.planes.push(hit);
            return view;
        }
        let view = table_texture(device, queue, cov, w, h);
        if self.planes.len() >= MASK_PLANES {
            self.planes.remove(0);
        }
        self.planes.push((cov.clone(), view.clone()));
        view
    }

    /// Prepare the passes of `fx`, whose source draw uses `source_bg` (a layer bind group whose
    /// uniforms map the frame onto the working image). None when the working image does not fit
    /// in a texture (the layer is then drawn without its effects — never larger than the source
    /// frame, which would not have uploaded either).
    pub(crate) fn job(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        fx: &LayerFx,
        source_bg: wgpu::BindGroup,
        dummy: &wgpu::TextureView,
    ) -> Option<FxJob> {
        let (w, h) = (fx.size.0.max(1), fx.size.1.max(1));
        let max = device.limits().max_texture_dimension_2d;
        if w > max || h > max {
            return None;
        }
        let all: Vec<Step> = fx.ops.iter().flat_map(|op| steps(op, w, h)).collect();
        // the picture the job starts from, when a step mixes with it at the end: never written to
        let base = all.iter().any(|s| s.mix_base).then_some(0usize);
        let n = 2 + all.iter().any(|s| s.combine) as usize + base.is_some() as usize;
        let targets = self.targets(device, (w, h), n);
        let views: Vec<wgpu::TextureView> = targets.iter().map(|t| t.1.clone()).collect();
        let mut dispatches = Vec::with_capacity(all.len());
        let mut cur = 0usize;
        // Unsharp: the image its blur started from (kept while its blur passes run)
        let mut orig: Option<usize> = None;
        for s in &all {
            // (an Unsharp without blur passes, radius 0, combines the original with itself)
            if (s.unsharp_blur || s.combine || s.reads_orig) && orig.is_none() {
                orig = Some(cur);
            }
            let keep = if s.unsharp_blur || s.combine || s.reads_orig { orig } else { None };
            let dst = (0..n).find(|k| *k != cur && Some(*k) != keep && Some(*k) != base)?;
            let kernel = match &s.plane {
                Some(cov) => Some(self.plane_texture(device, queue, cov, w, h)),
                None => (!s.kernel.is_empty()).then(|| kernel_texture(device, queue, &s.kernel)),
            };
            let aux = match (s.mix_base, base, s.combine || s.reads_orig, orig, &kernel) {
                (true, Some(b), ..) => views.get(b)?,
                (_, _, true, Some(o), _) => views.get(o)?,
                (.., Some(k)) => k,
                _ => dummy,
            };
            let table = match &s.table {
                Some((values, tw, th)) if (*tw as usize).checked_mul(*th as usize) == Some(values.len()) && *tw <= max && *th <= max && !values.is_empty() => {
                    // a plane of the picture's size is kept for the frames that share it; a small
                    // table changes from frame to frame and would push the planes out
                    // (a large table is a LUT, which also stays the same from frame to frame)
                    let kept = (*tw, *th) == (w, h) || values.len() > KEPT_TABLE;
                    Some(if kept { self.plane_texture(device, queue, values, *tw, *th) } else { table_texture(device, queue, values, *tw, *th) })
                }
                _ => None,
            };
            let mut bytes = Vec::with_capacity(128);
            for v in [s.code, w, h, 0].iter().chain(&s.i1) {
                bytes.extend_from_slice(&v.to_le_bytes());
            }
            for v in &s.p {
                bytes.extend_from_slice(&v.to_le_bytes());
            }
            let buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("fx-u"),
                size: bytes.len() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            queue.write_buffer(&buf, 0, &bytes);
            let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("fx"),
                layout: &self.bgl,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(views.get(cur)?) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(views.get(dst)?) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(aux) },
                    wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(table.as_ref().unwrap_or(dummy)) },
                ],
            });
            let groups = if s.run {
                let lines = if s.i1[2] != 0 { w } else { h };
                (lines.div_ceil(64), 1)
            } else {
                (w.div_ceil(16), h.div_ceil(16))
            };
            dispatches.push(Dispatch { run: s.run, bg, groups });
            cur = dst;
            if s.combine {
                orig = None;
            }
        }
        Some(FxJob {
            source_bg,
            source_target: views.first()?.clone(),
            dispatches,
            result: views.get(cur)?.clone(),
            result_texture: targets.get(cur)?.0.clone(),
        })
    }

    /// Record a job (source draw, then its compute passes) into `enc`.
    pub(crate) fn record(&self, enc: &mut wgpu::CommandEncoder, job: &FxJob) {
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("fx-source"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &job.source_target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.source);
            pass.set_bind_group(0, &job.source_bg, &[]);
            pass.draw(0..6, 0..1);
        }
        if job.dispatches.is_empty() {
            return;
        }
        let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("fx"), timestamp_writes: None });
        for d in &job.dispatches {
            pass.set_pipeline(if d.run { &self.run } else { &self.px });
            pass.set_bind_group(0, &d.bg, &[]);
            pass.dispatch_workgroups(d.groups.0, d.groups.1, 1);
        }
    }
}
