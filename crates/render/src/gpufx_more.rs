//! More standard effects with a GPU implementation: Luma Key, Rounded Crop, Drop Shadow, Noise and
//! Ultra Key, as variants of [`FxOp`](crate::gpufx::FxOp).
//!
//! Each is its evaluated parameters (`eval`, read as the CPU effect reads them) and the CPU
//! reference of the shader (`apply`), which repeats the arithmetic of the effect's own CPU code
//! in `effects` / `vfx` step by step. Tests hold the two together: `apply` must give the picture
//! `effects::apply` gives.

use filmcraft_color::{Matrix, hsl_to_rgb, linear_to_srgb, luma709, rgb_to_hsl, rgb_to_ycbcr, srgb_to_linear};
use filmcraft_geom::Affine;
use filmcraft_project::EffectInstance;
use rayon::prelude::*;

use crate::effects::{FxCtx, b, color, dec, enc, f, gaussian_boxes};
use crate::gpufx::Resample;
use crate::image::Image;
use crate::vfx::{bv, chv, cv, fv, lin, luma, over, smoothstep};

fn fin(v: &[f32]) -> bool {
    v.iter().all(|x| x.is_finite())
}

/// Luma Key: alpha from the encoded luma, a linear ramp from `cut` (transparent) to `th` (opaque),
/// measured from white with `invert`.
#[derive(Clone, Debug, PartialEq)]
pub struct LumaKey {
    pub th: f32,
    pub cut: f32,
    pub invert: bool,
}

impl LumaKey {
    pub(crate) fn eval(e: &EffectInstance, cx: &FxCtx) -> Self {
        Self { th: f(e, "threshold", cx) / 100.0, cut: f(e, "cutoff", cx) / 100.0, invert: b(e, "invert") }
    }

    pub(crate) fn gpu_ok(&self) -> bool {
        fin(&[self.th, self.cut])
    }

    pub(crate) fn apply(&self, img: &mut Image) {
        let (th, cut, invert) = (self.th, self.cut, self.invert);
        img.px.par_chunks_mut(4).for_each(|p| {
            if p[3] <= 0.0 {
                return;
            }
            let l = linear_to_srgb(luma709(p[0] / p[3], p[1] / p[3], p[2] / p[3]).max(0.0));
            let l = if invert { 1.0 - l } else { l };
            let a = if l <= cut {
                0.0
            } else if l >= th.max(cut + 1e-3) {
                1.0
            } else {
                (l - cut) / (th - cut).max(1e-3)
            };
            for v in p.iter_mut() {
                *v *= a;
            }
        });
    }
}

/// Lens Distortion: every pixel reads the picture (bilinear, transparent outside) at its offset
/// from the centre (`cx`, `cy`) scaled by `1 − k·r²`, `r` the offset over `norm` (half the
/// diagonal).
#[derive(Clone, Debug, PartialEq)]
pub struct LensDistortion {
    pub k: f64,
    pub cx: f64,
    pub cy: f64,
    pub norm: f64,
}

impl LensDistortion {
    pub(crate) fn eval(e: &EffectInstance, cx: &FxCtx, w: usize, h: usize) -> Self {
        let (w, h) = (w as f64, h as f64);
        Self {
            k: f(e, "curvature", cx) as f64 / 100.0,
            cx: w / 2.0 + f(e, "h_decentering", cx) as f64 / 100.0 * w / 2.0,
            cy: h / 2.0 + f(e, "v_decentering", cx) as f64 / 100.0 * h / 2.0,
            norm: (w / 2.0).hypot(h / 2.0),
        }
    }

    pub(crate) fn gpu_ok(&self) -> bool {
        [self.k, self.cx, self.cy, self.norm].iter().all(|v| v.is_finite() && v.abs() < 1e9) && self.norm > 0.0
    }

    pub(crate) fn apply(&self, img: &mut Image) {
        let (k, cxp, cyp, norm) = (self.k, self.cx, self.cy, self.norm);
        crate::effects::warp(img, |x, y| {
            let (dx, dy) = ((x - cxp) / norm, (y - cyp) / norm);
            let r2 = dx * dx + dy * dy;
            let s = 1.0 - k * r2;
            (cxp + dx * s * norm, cyp + dy * s * norm)
        });
    }
}

/// Emboss: the luma difference of the picture read (bilinear, edges repeated) at ±(`dx`, `dy`)
/// as a grey level, mixed with the picture by `blend`.
#[derive(Clone, Debug, PartialEq)]
pub struct Emboss {
    pub dx: f32,
    pub dy: f32,
    pub contrast: f32,
    pub blend: f32,
}

impl Emboss {
    pub(crate) fn eval(e: &EffectInstance, cx: &FxCtx) -> Self {
        let dir = (f(e, "direction", cx) as f64).to_radians();
        let relief = f(e, "relief", cx) * cx.px_scale.max(0.25);
        Self { dx: (dir.cos() as f32) * relief, dy: (-dir.sin() as f32) * relief, contrast: f(e, "contrast", cx) / 100.0, blend: f(e, "blend", cx) / 100.0 }
    }

    pub(crate) fn gpu_ok(&self) -> bool {
        fin(&[self.dx, self.dy, self.contrast, self.blend])
    }

    pub(crate) fn apply(&self, img: &mut Image) {
        let (dx, dy, contrast, blend) = (self.dx, self.dy, self.contrast, self.blend);
        let src = img.clone();
        let w = img.w;
        if w == 0 || img.h == 0 {
            return;
        }
        img.px.par_chunks_mut(w * 4).enumerate().for_each(|(y, row)| {
            for x in 0..w {
                let a = src.sample_bilinear_clamped(x as f32 + 0.5 + dx, y as f32 + 0.5 + dy);
                let bq = src.sample_bilinear_clamped(x as f32 + 0.5 - dx, y as f32 + 0.5 - dy);
                let v = srgb_to_linear((0.5 + (luma709(a[0], a[1], a[2]) - luma709(bq[0], bq[1], bq[2])) * contrast * 2.0).clamp(0.0, 1.0));
                let o = src.get(x, y);
                for k in 0..3 {
                    row[x * 4 + k] = v * o[3] * (1.0 - blend) + o[k] * blend;
                }
            }
        });
    }
}

/// VR Digital Glitch on a monoscopic frame: `rows` holds, for each row of the picture, its
/// sideways shift in pixels (wrapping around) and whether its band is glitching (1 or 0); a
/// glitching row reads red and blue `cs` pixels to either side, and odd rows darken by
/// `scan` × 0.6. Empty `rows`: the effect does nothing.
#[derive(Clone, Debug, PartialEq)]
pub struct DigitalGlitch {
    pub rows: std::sync::Arc<Vec<f32>>,
    pub cs: f32,
    pub scan: f32,
}

/// Tallest picture whose rows [`DigitalGlitch`] lists.
const GLITCH_MAX_ROWS: usize = 1 << 15;

impl DigitalGlitch {
    /// None for a stereoscopic layout (each view glitches on its own: the CPU's work) and for a
    /// picture taller than [`GLITCH_MAX_ROWS`].
    pub(crate) fn eval(e: &EffectInstance, cx: &FxCtx, w: usize, h: usize) -> Option<Self> {
        if chv(e, "frame_layout") != 0 || h > GLITCH_MAX_ROWS {
            return None;
        }
        let amp = fv(e, "amplitude", cx) / 100.0;
        let scan = fv(e, "scanlines", cx) / 100.0;
        // (also a NaN amplitude: the effect returns before doing anything)
        if amp <= 0.0 || amp.is_nan() {
            return Some(Self { rows: Default::default(), cs: 0.0, scan: 0.0 });
        }
        let cplx = fv(e, "distortion", cx).max(1.0);
        let rate = fv(e, "rate", cx) as f64;
        let colour = fv(e, "color", cx) / 100.0;
        let seed = fv(e, "seed", cx).max(0.0) as u64;
        let tick = (cx.seconds * rate).floor() as i64;
        let band = (h as f32 / cplx).max(1.0);
        let mut rows = Vec::with_capacity(h * 2);
        for y in 0..h {
            let b = (y as f32 / band) as i64;
            let active = crate::vfx::hash01(b, tick, 0, seed) < amp;
            let shift = if active { (crate::vfx::hash01(b, tick, 1, seed) - 0.5) * w as f32 * 0.3 * amp } else { 0.0 };
            rows.extend([shift, active as u8 as f32]);
        }
        Some(Self { rows: std::sync::Arc::new(rows), cs: colour * amp * 12.0, scan })
    }

    pub(crate) fn gpu_ok(&self) -> bool {
        fin(&[self.cs, self.scan]) && fin(&self.rows)
    }

    /// `vfx::immersive`'s `sample_wrap`: bilinear, wrapping sideways, rows clamped.
    fn sample_wrap(img: &Image, x: f64, y: f64) -> [f32; 4] {
        let (w, h) = (img.w as isize, img.h as isize);
        let fx = x - 0.5;
        let fy = (y - 0.5).clamp(0.0, (h - 1) as f64);
        let (x0, y0) = (fx.floor(), fy.floor());
        let (tx, ty) = ((fx - x0) as f32, (fy - y0) as f32);
        let (x0, y0) = (x0 as isize, y0 as isize);
        let g = |xx: isize, yy: isize| img.get(xx.rem_euclid(w) as usize, yy.clamp(0, h - 1) as usize);
        let (a, b, c, d) = (g(x0, y0), g(x0 + 1, y0), g(x0, y0 + 1), g(x0 + 1, y0 + 1));
        let mut o = [0.0; 4];
        for k in 0..4 {
            let top = a[k] + (b[k] - a[k]) * tx;
            let bot = c[k] + (d[k] - c[k]) * tx;
            o[k] = top + (bot - top) * ty;
        }
        o
    }

    pub(crate) fn apply(&self, img: &mut Image) {
        let (w, h) = (img.w, img.h);
        if w == 0 || self.rows.len() != h * 2 {
            return;
        }
        let (cs, scan) = (self.cs, self.scan);
        let src = img.clone();
        img.px.par_chunks_mut(w * 4).zip(self.rows.par_chunks(2)).enumerate().for_each(|(y, (row, r))| {
            let (shift, active) = (r[0], r[1] != 0.0);
            for x in 0..w {
                let (fx, fy) = (x as f64 + 0.5 + shift as f64, y as f64 + 0.5);
                let mut o = Self::sample_wrap(&src, fx, fy);
                if active && cs > 0.0 {
                    o[0] = Self::sample_wrap(&src, fx + cs as f64, fy)[0];
                    o[2] = Self::sample_wrap(&src, fx - cs as f64, fy)[2];
                }
                if scan > 0.0 && y % 2 == 1 {
                    for c in &mut o[..3] {
                        *c *= 1.0 - scan * 0.6;
                    }
                }
                row[x * 4..x * 4 + 4].copy_from_slice(&o);
            }
        });
    }
}

/// Rounded Crop: the rectangle `x0..x1` × `y0..y1` with corner radius `rad`, an inward `feather`
/// (0: a one-pixel edge) and a `border` band of colour `bc` (linear) inside the edge.
#[derive(Clone, Debug, PartialEq)]
pub struct RoundedCrop {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    pub rad: f32,
    pub feather: f32,
    pub border: f32,
    pub bc: [f32; 3],
}

/// Signed distance to a rounded rectangle (`vfx::distort::round_rect_sdf`).
fn round_rect_sdf(px: f32, py: f32, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> f32 {
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let (hx, hy) = ((x1 - x0) / 2.0 - r, (y1 - y0) / 2.0 - r);
    let (qx, qy) = ((px - cx).abs() - hx, (py - cy).abs() - hy);
    let outside = qx.max(0.0).hypot(qy.max(0.0));
    outside + qx.max(qy).min(0.0) - r
}

impl RoundedCrop {
    pub(crate) fn eval(e: &EffectInstance, cx: &FxCtx, w: usize, h: usize) -> Self {
        let (w, h) = (w as f32, h as f32);
        let (x0, y0, x1, y1) =
            (fv(e, "left", cx) / 100.0 * w, fv(e, "top", cx) / 100.0 * h, w - fv(e, "right", cx) / 100.0 * w, h - fv(e, "bottom", cx) / 100.0 * h);
        let rad = (fv(e, "radius", cx) * cx.px_scale).min(((x1 - x0).min(y1 - y0) / 2.0).max(0.0));
        Self { x0, y0, x1, y1, rad, feather: fv(e, "feather", cx) * cx.px_scale, border: fv(e, "border", cx) * cx.px_scale, bc: lin(cv(e, "border_color", cx)) }
    }

    pub(crate) fn gpu_ok(&self) -> bool {
        fin(&[self.x0, self.y0, self.x1, self.y1, self.rad, self.feather, self.border]) && fin(&self.bc)
    }

    pub(crate) fn apply(&self, img: &mut Image) {
        let Self { x0, y0, x1, y1, rad, feather: fe, border, bc } = *self;
        let wi = img.w;
        img.px.par_chunks_mut(wi * 4).enumerate().for_each(|(y, row)| {
            for (x, p) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let d = round_rect_sdf(x as f32 + 0.5, y as f32 + 0.5, x0, y0, x1, y1, rad);
                if border > 0.0 {
                    let k = (0.5 - d).clamp(0.0, 1.0) - (0.5 - d - border).clamp(0.0, 1.0);
                    if k > 0.0 {
                        over(p, [bc[0] * k, bc[1] * k, bc[2] * k, k]);
                    }
                }
                let a = if fe > 0.0 { (-d / fe).clamp(0.0, 1.0) } else { (0.5 - d).clamp(0.0, 1.0) };
                if a < 1.0 {
                    for v in p.iter_mut() {
                        *v *= a;
                    }
                }
            }
        });
    }
}

/// Drop Shadow: the picture's alpha moved by `shift`, tinted `color` (linear) at `opacity`,
/// blurred by the box radii, with the picture over it unless `only`.
#[derive(Clone, Debug, PartialEq)]
pub struct DropShadow {
    pub color: [f32; 3],
    pub opacity: f32,
    pub shift: Resample,
    pub rx: Vec<u32>,
    pub ry: Vec<u32>,
    pub only: bool,
}

impl DropShadow {
    pub(crate) fn eval(e: &EffectInstance, cx: &FxCtx, w: usize, h: usize) -> Self {
        let c = color(e, "color", cx);
        let dir = (f(e, "direction", cx) as f64).to_radians();
        let dist = f(e, "distance", cx) as f64 * cx.px_scale as f64;
        let soft = f(e, "softness", cx) * cx.px_scale * 0.5;
        let (dx, dy) = (dir.sin() * dist, -dir.cos() * dist);
        let (rx, ry) = if soft > 0.3 { gaussian_boxes(w, h, soft, soft) } else { (Vec::new(), Vec::new()) };
        Self {
            color: dec([c[0], c[1], c[2]]),
            opacity: f(e, "opacity", cx) / 100.0,
            shift: crate::gpufx::resample(w, h, &Affine::translate(dx, dy), 1.0),
            rx,
            ry,
            only: b(e, "only"),
        }
    }

    pub(crate) fn gpu_ok(&self) -> bool {
        fin(&self.color) && self.opacity.is_finite() && self.shift.gpu_ok()
    }

    pub(crate) fn apply(&self, img: &mut Image) {
        let (lc, op) = (self.color, self.opacity);
        let mut sh = match &self.shift.inv {
            Some(inv) => crate::gpufx::resample_cpu(img, inv, self.shift.rect),
            None => Image::new(img.w, img.h),
        };
        sh.px.par_chunks_mut(4).for_each(|p| {
            let a = p[3] * op;
            p.copy_from_slice(&[lc[0] * a, lc[1] * a, lc[2] * a, a]);
        });
        crate::effects::box_blur(&mut sh, &self.rx, &self.ry, false);
        if !self.only {
            crate::blend::composite(&mut sh, img, 1.0, crate::blend::Blend::Normal);
        }
        *img = sh;
    }
}

/// Noise: per-frame random noise of `amount` added in display space, one value per `grain`×`grain`
/// block, per channel with `colored`, clamped to 0…1 with `clip`. `seed` is the frame's.
#[derive(Clone, Debug, PartialEq)]
pub struct Noise {
    pub amount: f32,
    pub colored: bool,
    pub clip: bool,
    pub grain: f32,
    pub seed: u64,
}

impl Noise {
    /// The seeds of the second and third channel of colour noise are this far from the first.
    pub const CHANNEL_SEEDS: [u64; 2] = [7777, 99_991];

    pub(crate) fn eval(e: &EffectInstance, cx: &FxCtx) -> Self {
        Self {
            amount: fv(e, "amount", cx) / 100.0,
            colored: bv(e, "color"),
            clip: bv(e, "clip"),
            grain: (fv(e, "grain_size", cx).max(1.0) * cx.px_scale).max(1.0),
            seed: crate::effects::noise_frame_seed(cx),
        }
    }

    pub(crate) fn gpu_ok(&self) -> bool {
        fin(&[self.amount, self.grain])
    }

    /// Whether the effect changes anything (`vfx::color::noise` returns early otherwise).
    pub fn is_active(&self) -> bool {
        self.amount > 0.0 || self.amount.is_nan()
    }

    pub(crate) fn apply(&self, img: &mut Image) {
        if !self.is_active() {
            return;
        }
        let Self { amount: amt, colored, clip, grain, seed: frame } = *self;
        let w = img.w;
        img.px.par_chunks_mut(w * 4).enumerate().for_each(|(y, row)| {
            for (x, p) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let a = p[3];
                if a <= 0.0 {
                    continue;
                }
                let (gx, gy) = ((x as f32 / grain) as usize, (y as f32 / grain) as usize);
                let h = |z: u64| crate::effects::hash3(gx, gy, z) - 0.5;
                let n0 = h(frame);
                let ns = if colored { [n0, h(frame.wrapping_add(Self::CHANNEL_SEEDS[0])), h(frame.wrapping_add(Self::CHANNEL_SEEDS[1]))] } else { [n0; 3] };
                let c = enc([p[0] / a, p[1] / a, p[2] / a]);
                let mut o = [c[0] + ns[0] * amt, c[1] + ns[1] * amt, c[2] + ns[2] * amt];
                if clip {
                    o = o.map(|v| v.clamp(0.0, 1.0));
                }
                let o = dec(o);
                p[0] = o[0] * a;
                p[1] = o[1] * a;
                p[2] = o[2] * a;
            }
        });
    }
}

/// Ultra Key (`vfx::color::ultra_key`): a matte from the chroma distance to the key colour,
/// its cleanup (choke, soften, contrast), spill suppression and colour correction.
#[derive(Clone, Debug, PartialEq)]
pub struct UltraKey {
    /// The key colour as Y'CbCr (BT.709) and the squared length of its chroma.
    pub key: [f32; 3],
    pub kmag2: f32,
    pub tol: f32,
    pub gain: f32,
    pub highlight: f32,
    pub shadow: f32,
    pub pedestal: f32,
    /// Choke: the box radii of the matte's blur (rows, columns) and the amount taken off it.
    pub choke: Option<(Vec<u32>, Vec<u32>, f32)>,
    /// Soften: the box radii of the matte's blur.
    pub soften: (Vec<u32>, Vec<u32>),
    pub contrast: f32,
    pub mid: f32,
    pub spill: f32,
    pub desat: f32,
    pub range: f32,
    pub spill_luma: f32,
    /// Colour correction (saturation factor, hue turn, luminance factor), None when neutral.
    pub cc: Option<[f32; 3]>,
    /// The key colour's largest channel.
    pub dom: u32,
    /// 0 composite, 1 alpha channel, 2 colour channel.
    pub output: u32,
}

impl UltraKey {
    pub(crate) fn eval(e: &EffectInstance, cx: &FxCtx, w: usize, h: usize) -> Self {
        let kc = cv(e, "key_color", cx);
        let k = rgb_to_ycbcr(kc[0], kc[1], kc[2], Matrix::Bt709);
        let preset = filmcraft_project::effect::ultra_key_setting(chv(e, "setting"));
        let pf = |id: &str| preset.and_then(|rows| rows.iter().find(|(k, _)| *k == id).map(|(_, v)| *v as f32)).unwrap_or_else(|| fv(e, id, cx));
        let transparency = pf("transparency") / 100.0;
        let choke = pf("choke") / 100.0;
        let soften = pf("soften") / 100.0;
        let blur = |sigma: f32| if sigma > 0.3 { gaussian_boxes(w, h, sigma, sigma) } else { (Vec::new(), Vec::new()) };
        let (cc_s, cc_h, cc_l) = (fv(e, "cc_saturation", cx) / 100.0, fv(e, "cc_hue", cx) / 360.0, fv(e, "cc_luminance", cx) / 100.0);
        let cc = (cc_s - 1.0).abs() > 1e-4 || cc_h.abs() > 1e-6 || (cc_l - 1.0).abs() > 1e-4;
        Self {
            key: k,
            kmag2: (k[1] * k[1] + k[2] * k[2]).max(1e-6),
            tol: 0.08 + pf("tolerance") / 100.0 * 0.5,
            gain: 1.0 + 2.0 * transparency,
            highlight: pf("highlight") / 100.0,
            shadow: pf("shadow") / 100.0,
            pedestal: (pf("pedestal") / 100.0 * 0.3).min(0.9),
            choke: (choke > 0.0).then(|| {
                let (rx, ry) = blur((choke * 4.0 * cx.px_scale).max(0.5));
                (rx, ry, choke * 0.5)
            }),
            soften: if soften > 0.0 { blur(soften * 10.0 * cx.px_scale) } else { (Vec::new(), Vec::new()) },
            contrast: pf("contrast") / 100.0,
            mid: pf("mid_point") / 100.0,
            spill: pf("spill") / 100.0,
            desat: pf("desaturate") / 100.0,
            range: pf("range") / 100.0,
            spill_luma: pf("spill_luma") / 100.0,
            cc: cc.then_some([cc_s, cc_h, cc_l]),
            dom: if kc[1] >= kc[0] && kc[1] >= kc[2] {
                1
            } else if kc[2] >= kc[0] {
                2
            } else {
                0
            },
            output: chv(e, "output"),
        }
    }

    pub(crate) fn gpu_ok(&self) -> bool {
        fin(&self.key)
            && fin(&[self.kmag2, self.tol, self.gain, self.highlight, self.shadow, self.pedestal, self.contrast, self.mid])
            && fin(&[self.spill, self.desat, self.range, self.spill_luma])
            && self.choke.as_ref().is_none_or(|c| c.2.is_finite())
            && self.cc.is_none_or(|c| fin(&c))
    }

    /// The matte of a premultiplied pixel, before cleanup.
    fn matte(&self, p: &[f32]) -> f32 {
        if p[3] <= 1e-6 {
            return 0.0;
        }
        let k = self.key;
        let v = enc(Image::unpremul([p[0], p[1], p[2], p[3]]));
        let y = rgb_to_ycbcr(v[0], v[1], v[2], Matrix::Bt709);
        let proj = (y[1] * k[1] + y[2] * k[2]) / self.kmag2;
        let perp = ((y[1] - proj * k[1]).powi(2) + (y[2] - proj * k[2]).powi(2)).sqrt() / self.kmag2.sqrt();
        let keyness = proj.clamp(0.0, 1.5) * (1.0 - smoothstep(0.0, self.tol, perp));
        let mut m = (1.0 - keyness * self.gain).clamp(0.0, 1.0);
        m += (y[0] - k[0]).max(0.0) * self.highlight * 2.0 * keyness.min(1.0);
        m += (k[0] - y[0]).max(0.0) * self.shadow * 2.0 * keyness.min(1.0);
        ((m.clamp(0.0, 1.0) - self.pedestal) / (1.0 - self.pedestal)).clamp(0.0, 1.0)
    }

    /// A matte plane blurred by box passes, as `vfx::blur_plane` blurs it.
    fn blurred(alpha: &[f32], w: usize, h: usize, rx: &[u32], ry: &[u32]) -> Vec<f32> {
        if rx.is_empty() && ry.is_empty() {
            return alpha.to_vec();
        }
        let mut im = Image { w, h, px: alpha.iter().flat_map(|&v| [v, 0.0, 0.0, 0.0]).collect() };
        crate::effects::box_blur(&mut im, rx, ry, false);
        im.px.as_chunks::<4>().0.iter().map(|p| p[0]).collect()
    }

    pub(crate) fn apply(&self, img: &mut Image) {
        let mut alpha: Vec<f32> = img.px.par_chunks(4).map(|p| self.matte(p)).collect();
        if let Some((rx, ry, c)) = &self.choke {
            let c = *c;
            let blurred = Self::blurred(&alpha, img.w, img.h, rx, ry);
            alpha.par_iter_mut().zip(blurred.par_iter()).for_each(|(a, &bb)| *a = a.min(((bb - c) / (1.0 - c)).clamp(0.0, 1.0)));
        }
        alpha = Self::blurred(&alpha, img.w, img.h, &self.soften.0, &self.soften.1);
        let (contrast, mid) = (self.contrast, self.mid);
        if contrast > 0.0 {
            let g = 1.0 + contrast * 4.0;
            alpha.par_iter_mut().for_each(|a| *a = ((*a - mid) * g + mid).clamp(0.0, 1.0));
        }
        let Self { spill, desat, range, spill_luma: sluma, cc, output, .. } = self.clone();
        let dom = (self.dom as usize).min(2);
        img.px.par_chunks_mut(4).zip(alpha.par_iter()).for_each(|(p, &a)| {
            let src_a = p[3];
            let mut v = enc(Image::unpremul([p[0], p[1], p[2], p[3]]));
            let (o1, o2) = (v[(dom + 1) % 3], v[(dom + 2) % 3]);
            let limit = o1.max(o2) * (1.0 - range) + (o1 + o2) * 0.5 * range;
            let excess = (v[dom] - limit).max(0.0);
            if excess > 0.0 && spill > 0.0 {
                let l0 = luma(v);
                v[dom] -= excess * spill;
                let l1 = luma(v);
                let lm = luma(v);
                v = v.map(|q| lm + (q - lm) * (1.0 - desat * (excess * 4.0).min(1.0)));
                let comp = (l0 - l1) * sluma;
                v = v.map(|q| q + comp);
            }
            if let Some([cc_s, cc_h, cc_l]) = cc {
                let mut hsl = rgb_to_hsl(v[0].clamp(0.0, 1.0), v[1].clamp(0.0, 1.0), v[2].clamp(0.0, 1.0));
                hsl[0] = (hsl[0] + cc_h).rem_euclid(1.0);
                hsl[1] = (hsl[1] * cc_s).clamp(0.0, 1.0);
                hsl[2] = (hsl[2] * cc_l).clamp(0.0, 1.0);
                v = hsl_to_rgb(hsl[0], hsl[1], hsl[2]);
            }
            let l = dec(v);
            match output {
                1 => {
                    let g = srgb_to_linear(a);
                    p.copy_from_slice(&[g * src_a, g * src_a, g * src_a, src_a]);
                }
                2 => p.copy_from_slice(&[l[0] * src_a, l[1] * src_a, l[2] * src_a, src_a]),
                _ => {
                    let na = a * src_a;
                    p.copy_from_slice(&[l[0] * na, l[1] * na, l[2] * na, na]);
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use filmcraft_project::{ParamValue, find_effect};
    use filmcraft_time::Tick;

    use super::*;
    use crate::gpufx::FxOp;

    fn cx(px_scale: f32) -> FxCtx<'static> {
        FxCtx {
            t: Tick(123_456_789_012),
            px_scale,
            mask_scale: px_scale,
            seconds: 0.5,
            timecode: "",
            clip_name: "",
            project: None,
            env: None,
            frame_rate: 60.0,
            working: filmcraft_color::WorkingSpace::Rec709,
        }
    }

    /// A picture with a green-screen area, a subject, partial alpha and a transparent band.
    fn picture(w: usize, h: usize) -> Image {
        let mut img = Image::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let (u, v) = (x as f32 / w as f32, y as f32 / h as f32);
                let c = if (u - 0.5).hypot(v - 0.5) < 0.28 {
                    [0.75 - 0.4 * u, 0.3 + 0.3 * v, 0.2 + 0.5 * u]
                } else {
                    [0.08 + 0.1 * v, 0.7 + 0.2 * u, 0.1 + 0.1 * u]
                };
                let a = match y % 9 {
                    0 => 0.0,
                    1 => 0.5,
                    _ => 1.0,
                };
                let l = dec(c);
                img.px[(y * w + x) * 4..][..4].copy_from_slice(&[l[0] * a, l[1] * a, l[2] * a, a]);
            }
        }
        img
    }

    fn effect(id: &str, params: &[(&str, ParamValue)]) -> EffectInstance {
        let mut e = find_effect(id).unwrap_or_else(|| panic!("{id}")).instance();
        for (k, v) in params {
            e.params.get_mut(*k).unwrap_or_else(|| panic!("{id}.{k}")).value = v.clone();
        }
        e
    }

    fn fl(v: f64) -> ParamValue {
        ParamValue::Float(v)
    }

    pub(crate) fn cases() -> Vec<(&'static str, Vec<(&'static str, ParamValue)>)> {
        let green = ParamValue::Color([0.0, 1.0, 0.0, 1.0]);
        let custom = ParamValue::Choice(3);
        vec![
            ("emboss", vec![]),
            ("emboss", vec![("direction", fl(110.0)), ("relief", fl(3.5)), ("contrast", fl(160.0)), ("blend", fl(35.0))]),
            ("vr_digital_glitch", vec![]),
            ("vr_digital_glitch", vec![("amplitude", fl(100.0)), ("distortion", fl(9.0)), ("color", fl(80.0)), ("scanlines", fl(50.0)), ("seed", fl(7.0))]),
            ("vr_digital_glitch", vec![("amplitude", fl(0.0))]),
            ("lens_distortion", vec![]),
            ("lens_distortion", vec![("curvature", fl(-40.0)), ("h_decentering", fl(20.0)), ("v_decentering", fl(-35.0))]),
            ("lens_distortion", vec![("curvature", fl(75.0))]),
            ("luma_key", vec![]),
            ("luma_key", vec![("threshold", fl(60.0)), ("cutoff", fl(20.0))]),
            ("luma_key", vec![("threshold", fl(30.0)), ("cutoff", fl(30.0)), ("invert", ParamValue::Bool(true))]),
            ("rounded_crop", vec![]),
            ("rounded_crop", vec![("left", fl(10.0)), ("top", fl(5.0)), ("right", fl(20.0)), ("bottom", fl(15.0)), ("radius", fl(12.0)), ("feather", fl(4.0))]),
            ("rounded_crop", vec![("radius", fl(500.0)), ("border", fl(3.0)), ("border_color", ParamValue::Color([1.0, 0.5, 0.0, 1.0]))]),
            ("drop_shadow", vec![]),
            ("drop_shadow", vec![("distance", fl(9.5)), ("direction", fl(40.0)), ("softness", fl(12.0)), ("opacity", fl(80.0))]),
            (
                "drop_shadow",
                vec![("distance", fl(4.0)), ("softness", fl(0.0)), ("only", ParamValue::Bool(true)), ("color", ParamValue::Color([0.2, 0.1, 0.6, 1.0]))],
            ),
            ("noise", vec![]),
            ("noise", vec![("amount", fl(35.0))]),
            ("noise", vec![("amount", fl(80.0)), ("color", ParamValue::Bool(false)), ("clip", ParamValue::Bool(false)), ("grain_size", fl(3.0))]),
            ("noise_legacy", vec![("amount", fl(20.0))]),
            ("ultra_key", vec![("key_color", green.clone())]),
            ("ultra_key", vec![("key_color", green.clone()), ("setting", ParamValue::Choice(2))]),
            ("ultra_key", vec![("key_color", green.clone()), ("setting", custom.clone()), ("choke", fl(30.0)), ("soften", fl(25.0)), ("contrast", fl(40.0))]),
            (
                "ultra_key",
                vec![
                    ("key_color", green.clone()),
                    ("setting", custom.clone()),
                    ("cc_saturation", fl(140.0)),
                    ("cc_hue", fl(60.0)),
                    ("cc_luminance", fl(90.0)),
                    ("spill", fl(90.0)),
                ],
            ),
            ("ultra_key", vec![("key_color", green.clone()), ("output", ParamValue::Choice(1)), ("setting", ParamValue::Choice(1))]),
            ("ultra_key", vec![("key_color", ParamValue::Color([0.1, 0.2, 0.9, 1.0])), ("output", ParamValue::Choice(2))]),
        ]
    }

    /// The op of every effect here renders the picture the effect's own CPU code renders, at full
    /// and reduced working sizes: `FxOp::apply` is then the reference of the shader and of the
    /// CPU renderer alike.
    #[test]
    fn the_ops_render_what_the_cpu_effects_render() {
        for px_scale in [1.0, 0.5] {
            let img = picture(96, 54);
            for (id, params) in cases() {
                let e = effect(id, &params);
                let cx = cx(px_scale);
                // the effect's own code, reached before `gpufx` or beside it
                let mut own = img.clone();
                if !crate::vfx::apply(&mut own, &e, &cx) {
                    crate::effects::apply_own(&mut own, &e, &cx);
                }
                let op = FxOp::eval(&e, &cx, img.w, img.h).unwrap_or_else(|| panic!("{id}: no op"));
                assert!(op.gpu_ok(), "{id} {params:?}");
                let mut got = img.clone();
                op.apply(&mut got);
                let max = got.px.iter().zip(&own.px).map(|(a, b)| (a - b).abs()).fold(0f32, f32::max);
                assert!(max < 1e-6, "{id} {params:?} at {px_scale}: max diff {max}");
            }
        }
    }

    /// Hostile parameters never panic, in evaluation or in the CPU reference.
    #[test]
    fn hostile_parameters_are_declined_or_bounded() {
        let img = picture(40, 24);
        let bad = [f64::NAN, f64::INFINITY, -f64::INFINITY, 1e30, -1e30];
        let numeric: [(&str, &[&str]); 8] = [
            ("emboss", &["direction", "relief", "contrast", "blend"]),
            ("vr_digital_glitch", &["amplitude", "distortion", "rate", "color", "scanlines", "seed"]),
            ("lens_distortion", &["curvature", "h_decentering", "v_decentering"]),
            ("luma_key", &["threshold", "cutoff"]),
            ("rounded_crop", &["left", "right", "radius", "feather", "border"]),
            ("drop_shadow", &["opacity", "direction", "distance", "softness"]),
            ("noise", &["amount", "grain_size"]),
            ("ultra_key", &["tolerance", "pedestal", "choke", "soften", "contrast", "cc_hue", "spill"]),
        ];
        for (id, params) in numeric {
            for name in params {
                for v in bad {
                    let mut set = vec![(*name, fl(v))];
                    if id == "ultra_key" {
                        set.push(("setting", ParamValue::Choice(3)));
                    }
                    let e = effect(id, &set);
                    for (w, h) in [(img.w, img.h), (1, 1)] {
                        let Some(op) = FxOp::eval(&e, &cx(1.0), w, h) else { continue };
                        let mut small = Image::new(w, h);
                        small.px.iter_mut().for_each(|p| *p = 0.5);
                        let mut out = if (w, h) == (img.w, img.h) { img.clone() } else { small };
                        op.apply(&mut out);
                        // (a clamp may absorb the value; what is left non-finite is declined)
                        let _ = op.gpu_ok();
                    }
                }
            }
        }
    }
}
