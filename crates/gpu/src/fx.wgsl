// FilmCraft GPU effect stage: the standard effects of `filmcraft_render::gpufx::FxOp`, one
// compute pass each, reading the working image (`src`, linear premultiplied RGBA f32) and writing
// the next one (`dst`). The math mirrors `FxOp::apply` (the CPU reference) operation by
// operation; parameters arrive evaluated (keyframes, defaults and clamps applied on the CPU).

struct U {
    i0: vec4<u32>,  // op, width, height, unused
    i1: vec4<u32>,  // integer parameters (box radius, repeat, steps, channel, rect…)
    p0: vec4<f32>,
    p1: vec4<f32>,
    p2: vec4<f32>,
    p3: vec4<f32>,
    p4: vec4<f32>,
    p5: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var src: texture_2d<f32>;
@group(0) @binding(2) var dst: texture_storage_2d<rgba32float, write>;
// Unsharp Mask: the image before blurring.
@group(0) @binding(3) var aux: texture_2d<f32>;
// A table of numbers for the pass (a coverage plane, curves, a LUT): one value per texel.
@group(0) @binding(4) var tbl: texture_2d<f32>;

const OP_BRIGHTNESS_CONTRAST: u32 = 1u;
const OP_PROC_AMP: u32 = 2u;
const OP_TINT: u32 = 3u;
const OP_BLACK_WHITE: u32 = 4u;
const OP_COLOR_BALANCE: u32 = 5u;
const OP_LEAVE_COLOR: u32 = 6u;
const OP_CHANGE_TO_COLOR: u32 = 7u;
const OP_COLOR_PASS: u32 = 8u;
const OP_GAMMA: u32 = 9u;
const OP_LEVELS: u32 = 10u;
const OP_EXTRACT: u32 = 11u;
const OP_INVERT: u32 = 12u;
const OP_INVERT_ALPHA: u32 = 13u;
const OP_POSTERIZE: u32 = 14u;
const OP_ASC_CDL: u32 = 15u;
const OP_CHANNEL_MIX: u32 = 16u;
const OP_COLOR_REPLACE: u32 = 17u;
const OP_ALPHA_ADJUST: u32 = 18u;
const OP_VIGNETTE: u32 = 19u;
const OP_BOX: u32 = 20u;
const OP_DIRECTIONAL: u32 = 22u;
const OP_UNSHARP: u32 = 23u;
const OP_CROP: u32 = 24u;
const OP_RESAMPLE: u32 = 25u;
const OP_HFLIP: u32 = 26u;
const OP_VFLIP: u32 = 27u;
const OP_MIRROR: u32 = 28u;
const OP_OFFSET: u32 = 29u;
const OP_VIDEO_LIMITER: u32 = 30u;
const OP_LUMETRI: u32 = 31u;
const OP_BOKEH_DOWN: u32 = 32u;
const OP_BOKEH_BOOST: u32 = 33u;
const OP_BOKEH: u32 = 34u;
const OP_BOKEH_UP: u32 = 35u;
const OP_THREE_WAY: u32 = 36u;
const OP_MASK: u32 = 100u;
const OP_RESAMPLE_ACC: u32 = 101u;
const OP_LUMA_KEY: u32 = 102u;
const OP_ROUNDED_CROP: u32 = 103u;
const OP_SHADOW: u32 = 104u;
const OP_OVER_ORIG: u32 = 105u;
const OP_NOISE: u32 = 106u;
const OP_KEY_MATTE: u32 = 107u;
const OP_KEY_CHOKE: u32 = 108u;
const OP_KEY_FINAL: u32 = 109u;
const OP_MIX_BASE: u32 = 110u;
const OP_LENS: u32 = 111u;
const OP_EMBOSS: u32 = 112u;
const OP_GLITCH: u32 = 113u;
const OP_GRADE_ADV: u32 = 114u;

fn size() -> vec2<i32> {
    return vec2<i32>(i32(u.i0.y), i32(u.i0.z));
}

fn ld(p: vec2<i32>) -> vec4<f32> {
    return textureLoad(src, p, 0);
}

// `Image::get_or_clear`
fn ld_or_clear(p: vec2<i32>) -> vec4<f32> {
    let s = size();
    if p.x < 0 || p.y < 0 || p.x >= s.x || p.y >= s.y {
        return vec4(0.0);
    }
    return textureLoad(src, p, 0);
}

// `Image::sample_bilinear`: pixel centres at +0.5, transparent outside.
fn sample_bilinear(x: f32, y: f32) -> vec4<f32> {
    let fx = x - 0.5;
    let fy = y - 0.5;
    let x0 = floor(fx);
    let y0 = floor(fy);
    let tx = fx - x0;
    let ty = fy - y0;
    let i = vec2<i32>(i32(x0), i32(y0));
    let a = ld_or_clear(i);
    let b = ld_or_clear(i + vec2(1, 0));
    let c = ld_or_clear(i + vec2(0, 1));
    let d = ld_or_clear(i + vec2(1, 1));
    let top = a + (b - a) * tx;
    let bot = c + (d - c) * tx;
    return top + (bot - top) * ty;
}

// `sample_bilinear` of the aux texture (the original of a sum of placements).
fn aux_or_clear(p: vec2<i32>) -> vec4<f32> {
    let s = size();
    if p.x < 0 || p.y < 0 || p.x >= s.x || p.y >= s.y {
        return vec4(0.0);
    }
    return textureLoad(aux, p, 0);
}

fn sample_bilinear_aux(x: f32, y: f32) -> vec4<f32> {
    let fx = x - 0.5;
    let fy = y - 0.5;
    let x0 = floor(fx);
    let y0 = floor(fy);
    let tx = fx - x0;
    let ty = fy - y0;
    let i = vec2<i32>(i32(x0), i32(y0));
    let a = aux_or_clear(i);
    let b = aux_or_clear(i + vec2(1, 0));
    let c = aux_or_clear(i + vec2(0, 1));
    let d = aux_or_clear(i + vec2(1, 1));
    let top = a + (b - a) * tx;
    let bot = c + (d - c) * tx;
    return top + (bot - top) * ty;
}

// `Image::sample_bilinear_clamped`
fn sample_clamped(x: f32, y: f32) -> vec4<f32> {
    let s = size();
    let fx = clamp(x - 0.5, 0.0, f32(s.x) - 1.0);
    let fy = clamp(y - 0.5, 0.0, f32(s.y) - 1.0);
    let x0 = i32(floor(fx));
    let y0 = i32(floor(fy));
    let x1 = min(x0 + 1, s.x - 1);
    let y1 = min(y0 + 1, s.y - 1);
    let tx = fx - f32(x0);
    let ty = fy - f32(y0);
    let a = ld(vec2(x0, y0));
    let b = ld(vec2(x1, y0));
    let c = ld(vec2(x0, y1));
    let d = ld(vec2(x1, y1));
    let top = a + (b - a) * tx;
    let bot = c + (d - c) * tx;
    return top + (bot - top) * ty;
}

// ---- Bokeh Blur works on the top-left `r` pixels of the textures (the halved image)

// `Image::get_clamped` within `r`
fn ld_in(p: vec2<i32>, r: vec2<i32>) -> vec4<f32> {
    return textureLoad(src, clamp(p, vec2(0), r - 1), 0);
}

// `Image::get_or_clear` within `r`
fn ld_or_clear_in(p: vec2<i32>, r: vec2<i32>) -> vec4<f32> {
    if p.x < 0 || p.y < 0 || p.x >= r.x || p.y >= r.y {
        return vec4(0.0);
    }
    return textureLoad(src, p, 0);
}

// `Image::sample_bilinear` within `r`
fn sample_bilinear_in(x: f32, y: f32, r: vec2<i32>) -> vec4<f32> {
    let fx = x - 0.5;
    let fy = y - 0.5;
    let x0 = floor(fx);
    let y0 = floor(fy);
    let tx = fx - x0;
    let ty = fy - y0;
    let i = vec2<i32>(i32(x0), i32(y0));
    let a = ld_or_clear_in(i, r);
    let b = ld_or_clear_in(i + vec2(1, 0), r);
    let c = ld_or_clear_in(i + vec2(0, 1), r);
    let d = ld_or_clear_in(i + vec2(1, 1), r);
    let top = a + (b - a) * tx;
    let bot = c + (d - c) * tx;
    return top + (bot - top) * ty;
}

// Bokeh Blur's passes (`vfx::bokeh`); i1.xy = the size of the image they read.
fn bokeh(op: u32, p: vec2<i32>) -> vec4<f32> {
    let r = vec2<i32>(i32(u.i1.x), i32(u.i1.y));
    switch op {
        case OP_BOKEH_DOWN: {
            // `Image::downsample2`: the mean of each 2x2 block (edge-clamped)
            let o = max(r / 2, vec2(1));
            if p.x >= o.x || p.y >= o.y {
                return vec4(0.0);
            }
            let q = p * 2;
            return (ld_in(q, r) + ld_in(q + vec2(1, 0), r) + ld_in(q + vec2(0, 1), r) + ld_in(q + vec2(1, 1), r)) * 0.25;
        }
        case OP_BOKEH_BOOST: {
            if p.x >= r.x || p.y >= r.y {
                return vec4(0.0);
            }
            let o = ld(p);
            if o.a <= 1e-6 {
                return o;
            }
            let l = luma709(enc(o.rgb / o.a));
            let k = 1.0 + u.p0.x * smoothstep_fx(u.p0.y, 1.0, l);
            return vec4(o.rgb * k, o.a);
        }
        case OP_BOKEH: {
            // the iris gather through its kernel (`vfx::bokeh_kernel`): i1.z entries in aux,
            // 64 a row, each (dx, dy, weight); the reads edge-clamped within the halved image
            if p.x >= r.x || p.y >= r.y {
                return vec4(0.0);
            }
            var acc = vec4(0.0);
            for (var k = 0u; k < u.i1.z; k++) {
                let c = textureLoad(aux, vec2(i32(k % 64u), i32(k / 64u)), 0);
                acc += ld_in(p + vec2<i32>(c.xy), r) * c.z;
            }
            return acc;
        }
        default: {
            // OP_BOKEH_UP: `Image::transformed` by the scale back to the whole image
            let pc = vec2<f32>(p) + 0.5;
            let uu = u.p0.x * pc.x + u.p0.z;
            let vv = u.p0.y * pc.y + u.p0.w;
            if uu < -1.0 || vv < -1.0 || uu > f32(r.x) + 1.0 || vv > f32(r.y) + 1.0 {
                return vec4(0.0);
            }
            return sample_bilinear_in(uu, vv, r);
        }
    }
}

// ---- colour helpers (`filmcraft_color`, `effects::enc` / `dec`)

fn linear_to_srgb1(v: f32) -> f32 {
    if v <= 0.0031308 {
        return v * 12.92;
    }
    return 1.055 * pow(v, 1.0 / 2.4) - 0.055;
}

fn srgb_to_linear1(v: f32) -> f32 {
    if v <= 0.04045 {
        return v / 12.92;
    }
    return pow((v + 0.055) / 1.055, 2.4);
}

fn enc(c: vec3<f32>) -> vec3<f32> {
    let m = max(c, vec3(0.0));
    return vec3(linear_to_srgb1(m.x), linear_to_srgb1(m.y), linear_to_srgb1(m.z));
}

fn dec(c: vec3<f32>) -> vec3<f32> {
    let m = clamp(c, vec3(0.0), vec3(1.0));
    return vec3(srgb_to_linear1(m.x), srgb_to_linear1(m.y), srgb_to_linear1(m.z));
}

fn luma709(c: vec3<f32>) -> f32 {
    return 0.2126 * c.x + 0.7152 * c.y + 0.0722 * c.z;
}

// `f32::powf` for x >= 0 (WGSL leaves pow(0, y) undefined).
fn powf(x: f32, y: f32) -> f32 {
    if x == 0.0 {
        if y == 0.0 {
            return 1.0;
        }
        if y > 0.0 {
            return 0.0;
        }
        return 3.0e38;
    }
    return pow(x, y);
}

// `f32::round`: halves away from zero (WGSL `round` rounds them to even).
fn round_away(x: f32) -> f32 {
    let a = abs(x);
    let f = floor(a);
    let r = select(f, f + 1.0, a - f >= 0.5);
    return select(r, -r, x < 0.0);
}

// `rem_euclid(1.0)`
fn fract_euclid(x: f32) -> f32 {
    return x - floor(x);
}

// `vfx::smoothstep` (not WGSL's: a degenerate edge pair divides by 1e-6)
fn smoothstep_fx(e0: f32, e1: f32, x: f32) -> f32 {
    let t = clamp((x - e0) / max(e1 - e0, 1e-6), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

fn rgb_to_hsl(c: vec3<f32>) -> vec3<f32> {
    let r = c.x; let g = c.y; let b = c.z;
    let mx = max(max(r, g), b);
    let mn = min(min(r, g), b);
    let l = (mx + mn) / 2.0;
    if abs(mx - mn) < 1e-7 {
        return vec3(0.0, 0.0, l);
    }
    let d = mx - mn;
    var s: f32;
    if l > 0.5 {
        s = d / (2.0 - mx - mn);
    } else {
        s = d / (mx + mn);
    }
    var h: f32;
    if mx == r {
        h = (g - b) / d + select(0.0, 6.0, g < b);
    } else if mx == g {
        h = (b - r) / d + 2.0;
    } else {
        h = (r - g) / d + 4.0;
    }
    return vec3(h / 6.0, s, l);
}

fn hsl_channel(p: f32, q: f32, t0: f32) -> f32 {
    let t = fract_euclid(t0);
    if t < 1.0 / 6.0 {
        return p + (q - p) * 6.0 * t;
    }
    if t < 0.5 {
        return q;
    }
    if t < 2.0 / 3.0 {
        return p + (q - p) * (2.0 / 3.0 - t) * 6.0;
    }
    return p;
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> vec3<f32> {
    if s <= 0.0 {
        return vec3(l);
    }
    var q: f32;
    if l < 0.5 {
        q = l * (1.0 + s);
    } else {
        q = l + s - l * s;
    }
    let p = 2.0 * l - q;
    return vec3(hsl_channel(p, q, h + 1.0 / 3.0), hsl_channel(p, q, h), hsl_channel(p, q, h - 1.0 / 3.0));
}

fn linear_to_enc_limiter(v: f32) -> f32 {
    if v <= 1.0 {
        return linear_to_srgb1(max(v, 0.0));
    }
    return 1.0 + (v - 1.0) / 2.4;
}

fn enc_to_linear_limiter(v: f32) -> f32 {
    if v <= 1.0 {
        return srgb_to_linear1(max(v, 0.0));
    }
    return 1.0 + (v - 1.0) * 2.4;
}

fn knee_limiter(v: f32, max_val: f32, comp: f32) -> f32 {
    let k = max_val * (1.0 - comp);
    if comp <= 0.0 || v <= k {
        return min(v, max_val);
    }
    let r = max_val - k;
    return k + r * (1.0 - exp(-(v - k) / r));
}

fn to_rgb_709(y: f32, cb: f32, cr: f32) -> vec3<f32> {
    let kr = 0.2126;
    let kb = 0.0722;
    let kg = 1.0 - kr - kb;
    let r = y + 2.0 * (1.0 - kr) * cr;
    let b = y + 2.0 * (1.0 - kb) * cb;
    let g = (y - kr * r - kb * b) / kg;
    return vec3(r, g, b);
}

fn limit_video(v: vec3<f32>, max_val: f32, comp: f32, axis: u32) -> vec3<f32> {
    let kr = 0.2126;
    let kb = 0.0722;
    let kg = 1.0 - kr - kb;
    var y = kr * v.x + kg * v.y + kb * v.z;
    var cb = (v.z - y) / (2.0 * (1.0 - kb));
    var cr = (v.x - y) / (2.0 * (1.0 - kr));

    if axis != 1u {
        y = knee_limiter(max(y, 0.0), max_val, comp);
    }

    if axis != 0u {
        let rgb = to_rgb_709(y, cb, cr);
        var s = 1.0;
        let d = rgb - vec3(y);

        if rgb.x > max_val && d.x > 1e-6 {
            s = min(s, max((max_val - y) / d.x, 0.0));
        }
        if rgb.x < 0.0 && d.x < -1e-6 {
            s = min(s, max(y / (-d.x), 0.0));
        }

        if rgb.y > max_val && d.y > 1e-6 {
            s = min(s, max((max_val - y) / d.y, 0.0));
        }
        if rgb.y < 0.0 && d.y < -1e-6 {
            s = min(s, max(y / (-d.y), 0.0));
        }

        if rgb.z > max_val && d.z > 1e-6 {
            s = min(s, max((max_val - y) / d.z, 0.0));
        }
        if rgb.z < 0.0 && d.z < -1e-6 {
            s = min(s, max(y / (-d.z), 0.0));
        }

        if axis == 3u && s < 1.0 - 1e-4 {
            s = min(knee_limiter(s, 1.0, max(comp, 0.03)), 1.0);
        }
        cb *= s;
        cr *= s;
    }

    let out_rgb = to_rgb_709(y, cb, cr);
    return clamp(out_rgb, vec3(0.0), vec3(max_val));
}

// ---- per-pixel colour effects on straight colour (`Image::map_rgb`)

fn color_op(op: u32, c: vec3<f32>) -> vec3<f32> {
    switch op {
        case OP_BRIGHTNESS_CONTRAST: {
            let br = u.p0.x; let co = u.p0.y;
            return dec((enc(c) - 0.5) * co + 0.5 + br);
        }
        case OP_PROC_AMP: {
            let br = u.p0.x; let co = u.p0.y; let hue = u.p0.z; let sat = u.p0.w;
            var hsl = rgb_to_hsl(enc(c));
            hsl.x = fract_euclid(hsl.x + hue);
            hsl.y = clamp(hsl.y * sat, 0.0, 1.0);
            return dec((hsl_to_rgb(hsl.x, hsl.y, hsl.z) - 0.5) * co + 0.5 + br);
        }
        case OP_TINT: {
            let bl = u.p0.xyz; let wh = u.p1.xyz; let amt = u.p0.w;
            let l = linear_to_srgb1(max(luma709(c), 0.0));
            let t = bl + (wh - bl) * l;
            let e = enc(c);
            return dec(e + (t - e) * amt);
        }
        case OP_BLACK_WHITE: {
            return vec3(luma709(c));
        }
        case OP_COLOR_BALANCE: {
            let e = enc(c);
            let l = 0.2126 * e.x + 0.7152 * e.y + 0.0722 * e.z;
            let ws = (1.0 - l) * (1.0 - l);
            let wh = l * l;
            let wm = 1.0 - ws - wh;
            var o = e + u.p0.xyz * ws + u.p1.xyz * max(wm, 0.0) + u.p2.xyz * wh;
            if u.i1.x != 0u {
                let l2 = 0.2126 * o.x + 0.7152 * o.y + 0.0722 * o.z;
                o = o + (l - l2);
            }
            return dec(o);
        }
        case OP_THREE_WAY: {
            if u.i1.x != 0u {
                return c;
            }
            let e = enc(c);
            let l = 0.2126 * e.x + 0.7152 * e.y + 0.0722 * e.z;
            let ts = clamp((l - u.p3.x) / max(u.p3.y - u.p3.x, 1e-4), 0.0, 1.0);
            let th = clamp((l - u.p3.z) / max(u.p3.w - u.p3.z, 1e-4), 0.0, 1.0);
            var ws = 1.0 - ts * ts * (3.0 - 2.0 * ts);
            var wh = th * th * (3.0 - 2.0 * th);
            var wm = max(1.0 - ws - wh, 0.0);
            let total = ws + wm + wh;
            ws = ws / total;
            wh = wh / total;
            wm = wm / total;
            let lvl = u.p0.w * ws + u.p1.w * wm + u.p2.w * wh;
            let o = e + u.p0.xyz * ws + u.p1.xyz * wm + u.p2.xyz * wh + vec3(lvl);
            let l2 = 0.2126 * o.x + 0.7152 * o.y + 0.0722 * o.z;
            // no upper clamp: HDR values keep their headroom
            let r = max(vec3(l2) + (o - vec3(l2)) * u.p4.x, vec3(0.0));
            return vec3(srgb_to_linear1(r.x), srgb_to_linear1(r.y), srgb_to_linear1(r.z));
        }
        case OP_LEAVE_COLOR: {
            let amt = u.p0.x; let kh = u.p0.y; let tol = u.p0.z; let soft = u.p0.w;
            let h = rgb_to_hsl(enc(c)).x;
            let d = min(abs(h - kh), 1.0 - abs(h - kh)) * 2.0;
            let keep = 1.0 - clamp((d - tol) / soft, 0.0, 1.0);
            let l = luma709(c);
            let k = amt * (1.0 - keep);
            return c + (l - c) * k;
        }
        case OP_CHANGE_TO_COLOR: {
            let fh = u.p0.x; let th = u.p0.y; let tol = u.p0.z; let soft = u.p0.w;
            var hsl = rgb_to_hsl(enc(c));
            let d = min(abs(hsl.x - fh), 1.0 - abs(hsl.x - fh));
            let w = 1.0 - clamp((d - tol) / soft, 0.0, 1.0);
            hsl.x = fract_euclid(hsl.x + (th - fh) * w);
            return dec(hsl_to_rgb(hsl.x, hsl.y, hsl.z));
        }
        case OP_COLOR_PASS: {
            let e = enc(c);
            let k = e - u.p0.xyz;
            let d = sqrt(k.x * k.x + k.y * k.y + k.z * k.z);
            let passes = (d <= u.p0.w * 1.2) != (u.i1.x != 0u);
            if passes {
                return c;
            }
            return vec3(luma709(c));
        }
        case OP_GAMMA: {
            let e = enc(c);
            let g = u.p0.x;
            return dec(vec3(powf(max(e.x, 0.0), g), powf(max(e.y, 0.0), g), powf(max(e.z, 0.0), g)));
        }
        case OP_LEVELS: {
            let ib = u.p0.x; let iw = u.p0.y; let ob = u.p0.z; let ow = u.p0.w; let g = u.p1.x;
            let v = clamp((enc(c) - ib) / (iw - ib), vec3(0.0), vec3(1.0));
            return dec(ob + vec3(powf(v.x, g), powf(v.y, g), powf(v.z, g)) * (ow - ob));
        }
        case OP_EXTRACT: {
            let lo = u.p0.x; let hi = u.p0.y; let soft = u.p0.z;
            let l = linear_to_srgb1(max(luma709(c), 0.0));
            // Hard cutoffs avoid both division by zero and an artificial narrow fade
            // that amplifies transfer-function rounding relative to the CPU reference.
            var inside = select(0.0, 1.0, l > lo && l < hi);
            if soft > 0.0 {
                inside = min(clamp((l - lo) / soft, 0.0, 1.0), clamp((hi - l) / soft, 0.0, 1.0));
            }
            return vec3(select(inside, 1.0 - inside, u.i1.x != 0u));
        }
        case OP_INVERT: {
            let ch = u.i1.x;
            let blend = u.p0.x;
            let e = enc(c);
            var o = e;
            if ch == 0u || ch == 1u { o.x = 1.0 - e.x; }
            if ch == 0u || ch == 2u { o.y = 1.0 - e.y; }
            if ch == 0u || ch == 3u { o.z = 1.0 - e.z; }
            return dec(o + (e - o) * blend);
        }
        case OP_POSTERIZE: {
            let n = u.p0.x;
            let e = enc(c) * n;
            return dec(vec3(round_away(e.x), round_away(e.y), round_away(e.z)) / n);
        }
        case OP_ASC_CDL: {
            let e = enc(c);
            let s = u.p0.xyz; let o = u.p1.xyz; let pw = max(u.p2.xyz, vec3(0.0)); let sat = u.p0.w;
            let q = clamp(e * s + o, vec3(0.0), vec3(1.0));
            let v = vec3(powf(q.x, pw.x), powf(q.y, pw.y), powf(q.z, pw.z));
            let l = luma709(v);
            return dec(clamp(l + sat * (v - l), vec3(0.0), vec3(1.0)));
        }
        case OP_CHANNEL_MIX: {
            let v = enc(c);
            let r = u.p0; let g = u.p1; let b = u.p2;
            return dec(clamp(vec3(r.x * v.x + r.y * v.y + r.z * v.z + r.w, g.x * v.x + g.y * v.y + g.z * v.z + g.w, b.x * v.x + b.y * v.y + b.z * v.z + b.w), vec3(0.0), vec3(1.0)));
        }
        case OP_COLOR_REPLACE: {
            let v = enc(c);
            let t = u.p0.xyz; let sim = u.p0.w;
            let dd = v - t;
            let d = sqrt(dd.x * dd.x + dd.y * dd.y + dd.z * dd.z);
            let k = 1.0 - smoothstep_fx(sim * 0.85, max(sim, 1e-4), d);
            if k <= 0.0 {
                return c;
            }
            var repl = u.p1.xyz;
            if u.i1.x == 0u {
                let rh = u.p2.xyz;
                repl = hsl_to_rgb(rh.x, rh.y, rgb_to_hsl(v).z);
            }
            return dec(v + (repl - v) * k);
        }
        default: {
            return c;
        }
    }
}

fn lumetri_op(c: vec3<f32>, p: vec2<i32>) -> vec3<f32> {
    let s = size();
    let creative_on = u.i1.x != 0u;
    let vignette_on = u.i1.y != 0u;

    // white balance + exposure in linear light
    let ge = u.p0.xyz;
    let lin = c * ge;
    var v = enc(lin);

    // whites / blacks: endpoints
    let b0 = u.p0.w;
    let w0 = u.p1.x;
    let denom = max(w0 - b0, 1e-3);
    v = (v - vec3(b0)) / denom;

    // highlights / shadows: luma-weighted lift/compress, hue preserving
    let sh_k = u.p1.y;
    let hl_k = u.p1.z;
    let l = luma709(v);
    let ws_val = clamp(1.0 - l, 0.0, 1.0);
    let ws = ws_val * ws_val * ws_val;
    let whl_val = clamp(l, 0.0, 1.0);
    let whl = whl_val * whl_val * whl_val;
    let nl = max(l + sh_k * ws + hl_k * whl, 0.0);
    // below a luma of 0.02 (LIFT_LUMA_FLOOR in effects.rs) the hue-preserving scale nl / l is held
    // at that luma's and the rest of the lift is added equally (lift_scale in effects.rs)
    var k = 1.0;
    if l > 1e-5 {
        k = min(nl / l, 1.0 + max(nl - l, 0.0) / 0.02);
    }
    let add = nl - max(l, 0.0) * k;
    v = v * k + vec3(add);

    // contrast: smooth S-curve around mid grey
    let contrast = u.p1.w;
    if abs(contrast) > 1e-4 {
        let k = 1.0 + contrast;
        let q = clamp(v, vec3(0.0), vec3(1.0));
        let sc = q * q * (vec3(3.0) - 2.0 * q);
        if k >= 1.0 {
            v = q + (sc - q) * (k - 1.0);
        } else {
            v = vec3(0.5) + (q - vec3(0.5)) * k;
        }
    }

    // faded film: lift blacks and compress
    let faded = u.p2.x;
    if faded > 0.0 {
        v = v * (1.0 - 0.25 * faded) + vec3(0.12 * faded);
    }

    // split tone
    if creative_on {
        let st_k = u.p4.xyz;
        let ht_k = u.p5.xyz;
        let l2 = clamp(luma709(v), 0.0, 1.0);
        v = v + st_k * (1.0 - l2) + ht_k * l2;
    }

    // saturation & vibrance
    let sat = u.p2.y;
    let vib = u.p2.z;
    let l3 = luma709(v);
    let cur_sat = max(max(v.x, v.y), v.z) - min(min(v.x, v.y), v.z);
    let sat_mult = sat * (1.0 + vib * (1.0 - clamp(cur_sat, 0.0, 1.0)));
    v = vec3(l3) + (v - vec3(l3)) * sat_mult;

    // vignette
    let va = u.p2.w;
    if vignette_on && abs(va) > 1e-4 {
        let vmid = u.p3.x;
        let vfeather = u.p3.y;
        let vround_aspect = u.p3.z;
        let w = f32(s.x);
        let h = f32(s.y);
        let nx = (f32(p.x) / w - 0.5) * 2.0 * vround_aspect;
        let ny = (f32(p.y) / h - 0.5) * 2.0;
        let d = sqrt(nx * nx + ny * ny) / 1.4142135623730951;
        let edge = clamp((d - vmid * 0.9) / (max(vfeather, 0.01) * 0.9), 0.0, 1.0);
        let e2 = edge * edge * (3.0 - 2.0 * edge);
        let k = 1.0 + va * 0.2 * e2;
        if va < 0.0 {
            v = v * max(k, 0.0);
        } else {
            v = v + (vec3(1.0) - v) * (k - 1.0);
        }
    }

    return dec(v);
}

// One output pixel of every op but the running-sum box blur.
// ---- Noise: `effects::hash3` in 64-bit integers made of two u32 (low, high)

// Full 64-bit product of two u32.
fn mul32(a: u32, b: u32) -> vec2<u32> {
    let a0 = a & 0xffffu; let a1 = a >> 16u; let b0 = b & 0xffffu; let b1 = b >> 16u;
    let p00 = a0 * b0; let p01 = a0 * b1; let p10 = a1 * b0; let p11 = a1 * b1;
    let mid = (p00 >> 16u) + (p01 & 0xffffu) + (p10 & 0xffffu);
    return vec2((mid << 16u) | (p00 & 0xffffu), p11 + (p01 >> 16u) + (p10 >> 16u) + (mid >> 16u));
}

// Wrapping 64-bit multiply.
fn mul64(a: vec2<u32>, b: vec2<u32>) -> vec2<u32> {
    let p = mul32(a.x, b.x);
    return vec2(p.x, p.y + a.x * b.y + a.y * b.x);
}

// Wrapping 64-bit add.
fn add64(a: vec2<u32>, b: vec2<u32>) -> vec2<u32> {
    let lo = a.x + b.x;
    return vec2(lo, a.y + b.y + select(0u, 1u, lo < a.x));
}

// 64-bit shift right by 0 < n < 32.
fn shr64(a: vec2<u32>, n: u32) -> vec2<u32> {
    return vec2((a.x >> n) | (a.y << (32u - n)), a.y >> n);
}

// `hash3(gx, gy, z)` with `zc` = z × 0x165667B19E3779F9.
fn noise_hash(gx: u32, gy: u32, zc: vec2<u32>) -> f32 {
    var h = mul64(vec2(gx, 0u), vec2(0x7F4A7C15u, 0x9E3779B9u)) ^ mul64(vec2(gy, 0u), vec2(0x27D4EB4Fu, 0xC2B2AE3Du)) ^ zc;
    h = h ^ shr64(h, 31u);
    h = mul64(h, vec2(0x1CE4E5B9u, 0xBF58476Du));
    h = h ^ shr64(h, 29u);
    return f32(h.y >> 8u) / 16777216.0;
}

// `(x as f32 / grain) as usize`: the grain cell of a pixel coordinate. The quotient is checked
// against products, which are rounded exactly where a GPU's division may not be.
fn grain_cell(x: i32, grain: f32) -> u32 {
    if grain == 1.0 {
        return u32(x);
    }
    let fx = f32(x);
    var g = u32(fx / grain);
    if f32(g + 1u) * grain <= fx {
        g += 1u;
    } else if g > 0u && f32(g) * grain > fx {
        g -= 1u;
    }
    return g;
}

// ---- Ultra Key (`gpufx_more::UltraKey`): p0 = key Y'CbCr, |key chroma|²; p1 = tolerance, gain,
// highlight, shadow; p2 = pedestal, choke amount, contrast, mid point; p3 = spill, desaturate,
// range, spill luma; p4 = colour correction saturation, hue, luminance, on; i1 = dominant key
// channel, output.

// `filmcraft_color::rgb_to_ycbcr` with the BT.709 matrix.
fn key_ycc(v: vec3<f32>) -> vec3<f32> {
    let kr = 0.2126; let kb = 0.0722;
    let y = kr * v.x + (1.0 - kr - kb) * v.y + kb * v.z;
    return vec3(y, (v.z - y) / (2.0 * (1.0 - kb)), (v.x - y) / (2.0 * (1.0 - kr)));
}

// `UltraKey::matte` of a premultiplied pixel.
fn key_matte(o: vec4<f32>) -> f32 {
    if o.a <= 1e-6 {
        return 0.0;
    }
    let y = key_ycc(enc(o.rgb / o.a));
    let k = u.p0.xyz; let kmag2 = u.p0.w;
    let proj = (y.y * k.y + y.z * k.z) / kmag2;
    let dx = y.y - proj * k.y; let dy = y.z - proj * k.z;
    let perp = sqrt(dx * dx + dy * dy) / sqrt(kmag2);
    let keyness = clamp(proj, 0.0, 1.5) * (1.0 - smoothstep_fx(0.0, u.p1.x, perp));
    var m = clamp(1.0 - keyness * u.p1.y, 0.0, 1.0);
    m += max(y.x - k.x, 0.0) * u.p1.z * 2.0 * min(keyness, 1.0);
    m += max(k.x - y.x, 0.0) * u.p1.w * 2.0 * min(keyness, 1.0);
    return clamp((clamp(m, 0.0, 1.0) - u.p2.x) / (1.0 - u.p2.x), 0.0, 1.0);
}

// Color Grading's curve `row` of the table at `x` (`lumetri_advanced`'s `sample`: 1024 entries
// over 0..1, linear between them).
fn curve_at(row: i32, x: f32) -> f32 {
    let q = clamp(x, 0.0, 1.0) * 1023.0;
    let i = i32(q);
    let j = min(i + 1, 1023);
    let a = textureLoad(tbl, vec2(i, row), 0).r;
    let b = textureLoad(tbl, vec2(j, row), 0).r;
    return a + (b - a) * (q - f32(i));
}

fn look_curve(v: vec3<f32>, k: f32) -> vec3<f32> {
    let x = clamp(v, vec3(0.0), vec3(1.0));
    return x + (x * x * (vec3(3.0) - 2.0 * x) - x) * k;
}

// `effects::apply_look`: the built-in Looks on the display-encoded colour.
fn apply_look(look: u32, c: vec3<f32>) -> vec3<f32> {
    let l = 0.2126 * c.x + 0.7152 * c.y + 0.0722 * c.z;
    let g = vec3(l);
    switch look {
        case 1u: {
            let shadow = vec3(0.0, 0.08, 0.1);
            let high = vec3(0.1, 0.04, -0.06);
            return look_curve(c + shadow * (1.0 - l) + high * l, 0.35);
        }
        case 2u: {
            let d = vec3(c.x * 1.06 + 0.02, c.y * 1.0 + 0.01, c.z * 0.9);
            return (d + (g - d) * 0.15) * 0.94 + vec3(0.04);
        }
        case 3u: {
            return look_curve(vec3(c.x * 0.9, c.y * 0.98, c.z * 1.1 + 0.02), 0.2);
        }
        case 4u: {
            return look_curve(c + (g - c) * 0.55, 0.6);
        }
        case 5u: {
            return vec3(0.08) + (c + (g - c) * 0.25) * 0.84;
        }
        case 6u: {
            return look_curve(g, 0.3);
        }
        case 7u: {
            return look_curve(vec3(c.x * 1.1 + 0.03, c.y * 1.02 + 0.01, c.z * 0.82), 0.15);
        }
        case 8u: {
            return vec3(c.x * 0.75, c.y * 0.85, c.z * 1.15) * 0.8;
        }
        default: {
            return c;
        }
    }
}

// `gpufx_more::GradeAdvanced::apply` on a straight linear colour: i1 = (look, curve mask, wheels
// in use), p0.x = look intensity, p1..p3 = shadows / midtones / highlights (wheel RGB, luminance).
fn grade_advanced(c: vec3<f32>) -> vec3<f32> {
    var v = enc(c);
    if u.i1.x > 0u {
        v = v + (apply_look(u.i1.x, v) - v) * u.p0.x;
    }
    if u.i1.z != 0u {
        let l = clamp(0.2126 * v.x + 0.7152 * v.y + 0.0722 * v.z, 0.0, 1.0);
        let wsh = (1.0 - l) * (1.0 - l);
        let whi = l * l;
        let wmid = max(1.0 - wsh - whi, 0.0);
        v = v + (u.p1.xyz * 0.3 + vec3(u.p1.w * 0.3)) * wsh;
        v = v + (u.p2.xyz * 0.3 + vec3(u.p2.w * 0.3)) * wmid;
        v = v * (vec3(1.0) + (u.p3.xyz * 0.5 + vec3(u.p3.w * 0.5)) * whi);
    }
    let m = u.i1.y;
    if (m & 1u) != 0u {
        v = vec3(curve_at(0, v.x), curve_at(0, v.y), curve_at(0, v.z));
    }
    if (m & 2u) != 0u {
        v.x = curve_at(1, v.x);
    }
    if (m & 4u) != 0u {
        v.y = curve_at(2, v.y);
    }
    if (m & 8u) != 0u {
        v.z = curve_at(3, v.z);
    }
    if (m & 0x1F0u) != 0u {
        var h = rgb_to_hsl(clamp(v, vec3(0.0), vec3(1.0)));
        let h0 = h.x;
        let s0 = h.y;
        let l0 = h.z;
        if (m & 32u) != 0u {
            h.x = fract_euclid(h.x + (curve_at(5, h0) - 0.5));
        }
        var sm = 1.0;
        if (m & 16u) != 0u {
            sm = sm * (curve_at(4, h0) * 2.0);
        }
        if (m & 128u) != 0u {
            sm = sm * (curve_at(7, l0) * 2.0);
        }
        if (m & 256u) != 0u {
            sm = sm * (curve_at(8, s0) * 2.0);
        }
        h.y = clamp(h.y * sm, 0.0, 1.0);
        if (m & 64u) != 0u {
            h.z = clamp(h.z + (curve_at(6, h0) - 0.5) * 0.5, 0.0, 1.0);
        }
        v = hsl_to_rgb(h.x, h.y, h.z);
    }
    return dec(v);
}

// The picture `shift` pixels to the right of `p`, bilinear between the two texels of its row,
// wrapping around sideways (`vfx::immersive`'s `sample_wrap` at a pixel centre).
fn glitch_read(p: vec2<i32>, shift: f32) -> vec4<f32> {
    let w = size().x;
    let whole = floor(shift);
    let x0 = p.x + i32(whole);
    let a = ld(vec2(((x0 % w) + w) % w, p.y));
    let b = ld(vec2((((x0 + 1) % w) + w) % w, p.y));
    return a + (b - a) * (shift - whole);
}

fn pixel(op: u32, p: vec2<i32>) -> vec4<f32> {
    let s = size();
    let pc = vec2<f32>(p) + 0.5;
    if op >= OP_BOKEH_DOWN && op <= OP_BOKEH_UP {
        return bokeh(op, p);
    }
    switch op {
        case OP_INVERT_ALPHA: {
            let o = ld(p);
            let a = o.a;
            let na = 1.0 - a;
            // (the CPU's threshold; `na / a` is only used when a > 1e-6)
            var k = 0.0;
            if a > 1e-6 {
                k = na / a;
            }
            let blend = u.p0.x;
            return vec4(o.rgb * k, na * (1.0 - blend) + a * blend);
        }
        case OP_LUMA_KEY: {
            // p0 = threshold, cutoff; i1.x = key out brights
            let o = ld(p);
            if o.a <= 0.0 {
                return o;
            }
            var l = linear_to_srgb1(max(luma709(o.rgb / o.a), 0.0));
            if u.i1.x != 0u {
                l = 1.0 - l;
            }
            let th = u.p0.x; let cut = u.p0.y;
            var a: f32;
            if l <= cut {
                a = 0.0;
            } else if l >= max(th, cut + 1e-3) {
                a = 1.0;
            } else {
                a = (l - cut) / max(th - cut, 1e-3);
            }
            return o * a;
        }
        case OP_ROUNDED_CROP: {
            // p0 = x0 y0 x1 y1; p1 = radius, feather, border; p2 = border colour
            let x0 = u.p0.x; let y0 = u.p0.y; let x1 = u.p0.z; let y1 = u.p0.w;
            let rad = u.p1.x; let fe = u.p1.y; let border = u.p1.z;
            let c = vec2((x0 + x1) / 2.0, (y0 + y1) / 2.0);
            let hh = vec2((x1 - x0) / 2.0 - rad, (y1 - y0) / 2.0 - rad);
            let q = abs(pc - c) - hh;
            let d = length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0) - rad;
            var o = ld(p);
            if border > 0.0 {
                let k = clamp(0.5 - d, 0.0, 1.0) - clamp(0.5 - d - border, 0.0, 1.0);
                if k > 0.0 {
                    o = vec4(u.p2.xyz * k, k) + o * (1.0 - k);
                }
            }
            var a: f32;
            if fe > 0.0 {
                a = clamp(-d / fe, 0.0, 1.0);
            } else {
                a = clamp(0.5 - d, 0.0, 1.0);
            }
            return select(o, o * a, a < 1.0);
        }
        case OP_SHADOW: {
            // Drop Shadow's shadow: the picture's alpha moved (the inverse map and rect of
            // OP_RESAMPLE, p1.z = opacity) in the colour p2
            if u.p1.w == 0.0 || u32(p.x) < u.i1.x || u32(p.x) >= u.i1.y || u32(p.y) < u.i1.z || u32(p.y) >= u.i1.w {
                return vec4(0.0);
            }
            let m0 = u.p0; let m1 = u.p1;
            let uu = m0.x * pc.x + m0.z * pc.y + m1.x;
            let vv = m0.y * pc.x + m0.w * pc.y + m1.y;
            if uu < -1.0 || vv < -1.0 || uu > f32(s.x) + 1.0 || vv > f32(s.y) + 1.0 {
                return vec4(0.0);
            }
            let a = sample_bilinear(uu, vv).a * m1.z;
            return vec4(u.p2.xyz * a, a);
        }
        case OP_OVER_ORIG: {
            // the original (aux) over src, as `blend::composite` mixes Normal
            let o = textureLoad(aux, p, 0);
            let under = ld(p);
            if o.a <= 0.0 {
                return under;
            }
            return o + under * (1.0 - o.a);
        }
        case OP_NOISE: {
            // p0 = amount, grain; i1 = seed product (low, high), colour noise, clip
            let o = ld(p);
            if o.a <= 0.0 {
                return o;
            }
            let gx = grain_cell(p.x, u.p0.y);
            let gy = grain_cell(p.y, u.p0.y);
            let zc = vec2(u.i1.x, u.i1.y);
            let n0 = noise_hash(gx, gy, zc) - 0.5;
            var ns = vec3(n0);
            if u.i1.z != 0u {
                // the seeds of the other two channels: + 7777 and + 99991, times the multiplier
                ns = vec3(n0, noise_hash(gx, gy, add64(zc, vec2(0x73506559u, 0x96E41AD7u))) - 0.5, noise_hash(gx, gy, add64(zc, vec2(0xD68A47DFu, 0xCEEFC0CCu))) - 0.5);
            }
            var c = enc(o.rgb / o.a) + ns * u.p0.x;
            if u.i1.w != 0u {
                c = clamp(c, vec3(0.0), vec3(1.0));
            }
            return vec4(dec(c) * o.a, o.a);
        }
        case OP_KEY_MATTE: {
            return vec4(key_matte(ld(p)), 0.0, 0.0, 0.0);
        }
        case OP_KEY_CHOKE: {
            // src: the blurred matte, aux: the picture
            let a = key_matte(textureLoad(aux, p, 0));
            let c = u.p2.y;
            return vec4(min(a, clamp((ld(p).r - c) / (1.0 - c), 0.0, 1.0)), 0.0, 0.0, 0.0);
        }
        case OP_KEY_FINAL: {
            // src: the matte, aux: the picture
            let o = textureLoad(aux, p, 0);
            var a = ld(p).r;
            if u.p2.z > 0.0 {
                let g = 1.0 + u.p2.z * 4.0;
                a = clamp((a - u.p2.w) * g + u.p2.w, 0.0, 1.0);
            }
            var v = vec3(0.0);
            if o.a > 1e-6 {
                v = o.rgb / o.a;
            }
            v = enc(v);
            let dom = u.i1.x;
            let e = vec3(f32(dom == 0u), f32(dom == 1u), f32(dom == 2u));
            let vd = dot(v, e);
            let o1 = dot(v, e.zxy);
            let o2 = dot(v, e.yzx);
            let spill = u.p3.x; let desat = u.p3.y; let range = u.p3.z; let sluma = u.p3.w;
            let limit = max(o1, o2) * (1.0 - range) + (o1 + o2) * 0.5 * range;
            let excess = max(vd - limit, 0.0);
            if excess > 0.0 && spill > 0.0 {
                let l0 = luma709(v);
                v = v - e * (excess * spill);
                let l1 = luma709(v);
                v = vec3(l1) + (v - vec3(l1)) * (1.0 - desat * min(excess * 4.0, 1.0));
                v = v + vec3((l0 - l1) * sluma);
            }
            if u.p4.w != 0.0 {
                var hsl = rgb_to_hsl(clamp(v, vec3(0.0), vec3(1.0)));
                hsl.x = fract_euclid(hsl.x + u.p4.y);
                hsl.y = clamp(hsl.y * u.p4.x, 0.0, 1.0);
                hsl.z = clamp(hsl.z * u.p4.z, 0.0, 1.0);
                v = hsl_to_rgb(hsl.x, hsl.y, hsl.z);
            }
            let l = dec(v);
            if u.i1.y == 1u {
                let g = srgb_to_linear1(a);
                return vec4(vec3(g * o.a), o.a);
            }
            if u.i1.y == 2u {
                return vec4(l * o.a, o.a);
            }
            let na = a * o.a;
            return vec4(l * na, na);
        }
        case OP_GRADE_ADV: {
            let o = ld(p);
            if o.a <= 1e-6 {
                return o;
            }
            return vec4(grade_advanced(o.rgb / o.a) * o.a, o.a);
        }
        case OP_EMBOSS: {
            // p0 = (dx, dy, contrast, blend)
            let a = sample_clamped(pc.x + u.p0.x, pc.y + u.p0.y);
            let b = sample_clamped(pc.x - u.p0.x, pc.y - u.p0.y);
            let v = srgb_to_linear1(clamp(0.5 + (luma709(a.rgb) - luma709(b.rgb)) * u.p0.z * 2.0, 0.0, 1.0));
            let o = ld(p);
            return vec4(vec3(v * o.a * (1.0 - u.p0.w)) + o.rgb * u.p0.w, o.a);
        }
        case OP_GLITCH: {
            // VR Digital Glitch: the row's (shift, glitching) from the table; p0 = (colour split,
            // scanlines). A read at x + shift is, between the texels floor(shift) and the next, by
            // the fraction of the shift: exact, as the CPU's `f64` position is.
            let shift = textureLoad(tbl, vec2(0, p.y), 0).r;
            let glitching = textureLoad(tbl, vec2(1, p.y), 0).r != 0.0;
            var o = glitch_read(p, shift);
            if glitching && u.p0.x > 0.0 {
                o.r = glitch_read(p, shift + u.p0.x).r;
                o.b = glitch_read(p, shift - u.p0.x).b;
            }
            if u.p0.y > 0.0 && (p.y & 1) == 1 {
                o = vec4(o.rgb * (1.0 - u.p0.y * 0.6), o.a);
            }
            return o;
        }
        case OP_LENS: {
            // Lens Distortion: p0 = (curvature, centre x, centre y, half diagonal)
            // (centre + offset × (1 − k·r²), written as the pixel minus its shift: the pixel's own
            // position is exact, so only the shift carries rounding)
            let c = u.p0.yz;
            let off = pc - c;
            let d = off / u.p0.w;
            let shift = off * (u.p0.x * dot(d, d));
            return sample_bilinear(pc.x - shift.x, pc.y - shift.y);
        }
        case OP_MIX_BASE: {
            // an adjustment layer over the picture below it (`aux`): `FxOp::mix_base`
            let o = textureLoad(aux, p, 0);
            var k = u.p0.x;
            if u.i1.x != 0u {
                k = k * textureLoad(tbl, p, 0).r;
            }
            return o + (ld(p) - o) * k;
        }
        case OP_MASK: {
            // opacity masks: `mask::scale_by` with the coverage plane in `aux`
            return ld(p) * textureLoad(aux, p, 0).r;
        }
        case OP_ALPHA_ADJUST: {
            let o = ld(p);
            let c = select(o.rgb / o.a, vec3(0.0), o.a <= 1e-6);
            var a = select(o.a, 1.0, u.i1.x != 0u);
            if u.i1.y != 0u {
                a = 1.0 - a;
            }
            a = clamp(a * u.p0.x, 0.0, 1.0);
            if u.i1.z != 0u {
                return vec4(vec3(srgb_to_linear1(a)), 1.0);
            }
            return vec4(c * a, a);
        }
        case OP_BOX: {
            // one box pass along x (i1.z = 0) or y (1): mean over 2r+1 pixels, clamped to the
            // edge (repeat) or skipping pixels outside, as `effects::box_rows`
            let r = i32(u.i1.x);
            let repeat = u.i1.y != 0u;
            let vertical = u.i1.z != 0u;
            let n = select(s.x, s.y, vertical);
            let x = select(p.x, p.y, vertical);
            var acc = vec4(0.0);
            for (var i = x - r; i <= x + r; i++) {
                if repeat || (i >= 0 && i < n) {
                    let j = clamp(i, 0, n - 1);
                    acc += ld(select(vec2(j, p.y), vec2(p.x, j), vertical));
                }
            }
            return acc * (1.0 / f32(2 * r + 1));
        }
        case OP_DIRECTIONAL: {
            let steps = u.i1.x;
            let dx = u.p0.x; let dy = u.p0.y;
            var acc = vec4(0.0);
            for (var i = 0u; i < steps; i++) {
                let t = f32(i) / f32(steps - 1u) - 0.5;
                acc += sample_clamped(pc.x + dx * t, pc.y + dy * t);
            }
            return acc / f32(steps);
        }
        case OP_UNSHARP: {
            // src: the blurred image, aux: the original. Each colour channel on its own, as
            // `effects::unsharp_boxes`, written as vector math: FXC (DX12 without dxcompiler.dll)
            // aborts on a write to `o[k]` inside a loop ("internal error: compilation aborted
            // unexpectedly"), which took the whole `fx` pipeline down with it.
            let o = textureLoad(aux, p, 0);
            let bq = ld(p);
            let amount = u.p0.x; let th = u.p0.y;
            let d = o.rgb - bq.rgb;
            let sharpened = max(o.rgb + d * amount, vec3(0.0));
            return vec4(select(o.rgb, sharpened, abs(d) >= vec3(th * o.a)), o.a);
        }
        case OP_CROP: {
            let x0 = u.p0.x; let x1 = u.p0.y; let y0 = u.p0.z; let y1 = u.p0.w; let fe = u.p1.x;
            let d = min(min(pc.x - x0, x1 - pc.x), min(pc.y - y0, y1 - pc.y));
            var a: f32;
            if fe > 0.0 {
                a = clamp(d / fe, 0.0, 1.0);
            } else {
                a = clamp(d + 0.5, 0.0, 1.0);
            }
            let o = ld(p);
            return select(o, o * a, a < 1.0);
        }
        case OP_RESAMPLE: {
            // inverse map p0 = (a b c d), p1 = (e f opacity valid); rect i1 = x0 x1 y0 y1
            if u.p1.w == 0.0 || u32(p.x) < u.i1.x || u32(p.x) >= u.i1.y || u32(p.y) < u.i1.z || u32(p.y) >= u.i1.w {
                return vec4(0.0);
            }
            let m0 = u.p0; let m1 = u.p1;
            let uu = m0.x * pc.x + m0.z * pc.y + m1.x;
            let vv = m0.y * pc.x + m0.w * pc.y + m1.y;
            if uu < -1.0 || vv < -1.0 || uu > f32(s.x) + 1.0 || vv > f32(s.y) + 1.0 {
                return vec4(0.0);
            }
            return sample_bilinear(uu, vv) * m1.z;
        }
        case OP_RESAMPLE_ACC: {
            // one placement of the original (aux) added to the sum so far (src): the inverse map
            // and rect of OP_RESAMPLE, p2 = (first pass, 1 / placements)
            var acc = vec4(0.0);
            if u.p2.x == 0.0 {
                acc = ld(p);
            }
            if u.p1.w == 0.0 || u32(p.x) < u.i1.x || u32(p.x) >= u.i1.y || u32(p.y) < u.i1.z || u32(p.y) >= u.i1.w {
                return acc;
            }
            let m0 = u.p0; let m1 = u.p1;
            let uu = m0.x * pc.x + m0.z * pc.y + m1.x;
            let vv = m0.y * pc.x + m0.w * pc.y + m1.y;
            if uu < -1.0 || vv < -1.0 || uu > f32(s.x) + 1.0 || vv > f32(s.y) + 1.0 {
                return acc;
            }
            return acc + sample_bilinear_aux(uu, vv) * (m1.z * u.p2.y);
        }
        case OP_HFLIP: {
            return ld(vec2(s.x - 1 - p.x, p.y));
        }
        case OP_VFLIP: {
            return ld(vec2(p.x, s.y - 1 - p.y));
        }
        case OP_MIRROR: {
            let c = u.p0.xy; let nrm = u.p0.zw;
            let d = (pc.x - c.x) * nrm.x + (pc.y - c.y) * nrm.y;
            if d > 0.0 {
                return sample_bilinear(pc.x - 2.0 * d * nrm.x, pc.y - 2.0 * d * nrm.y);
            }
            return ld(p);
        }
        case OP_OFFSET: {
            let w = f32(s.x); let h = f32(s.y);
            // dx, dy in 0..w / 0..h, so the shifted coordinate is within (-w, w + 0.5)
            var uu = pc.x - u.p0.x;
            var vv = pc.y - u.p0.y;
            if uu < 0.0 { uu += w; }
            if uu >= w { uu -= w; }
            if vv < 0.0 { vv += h; }
            if vv >= h { vv -= h; }
            let q = sample_clamped(uu, vv);
            let o = ld(p);
            return q + (o - q) * u.p0.z;
        }
        case OP_VIGNETTE: {
            let o = ld(p);
            let a = o.a;
            if a <= 1e-6 {
                return o;
            }
            let amt = u.p0.x;
            if abs(amt) < 1e-5 {
                return o;
            }
            let mid = u.p0.y;
            let round = u.p0.z;
            let feather = u.p0.w;
            let tgt = u.p1.xyz;
            let w = f32(s.x);
            let h = f32(s.y);
            let aspect = w / h;
            var nx = pc.x / w * 2.0 - 1.0;
            let ny = pc.y / h * 2.0 - 1.0;
            if round > 0.0 {
                nx *= 1.0 + (aspect - 1.0) * round;
            }
            let p_exp = select(2.0, 2.0 + (-round) * 6.0, round < 0.0);
            let d = powf(powf(abs(nx), p_exp) + powf(abs(ny), p_exp), 1.0 / p_exp) / powf(2.0, 1.0 / p_exp);
            let edge = smoothstep_fx(mid - feather * 0.5, mid + feather * 0.5, d);
            let c = o.rgb / a;
            let ec = enc(c);
            let res = dec(ec + (tgt - ec) * (edge * abs(amt)));
            return vec4(res * a, a);
        }
        case OP_VIDEO_LIMITER: {
            let o = ld(p);
            let a = o.a;
            if a <= 1e-6 {
                return o;
            }
            let c = o.rgb / a;
            let v = vec3(linear_to_enc_limiter(c.x), linear_to_enc_limiter(c.y), linear_to_enc_limiter(c.z));
            let max_val = u.p0.x;
            let comp = u.p0.y;
            let axis = u.i1.x;
            let warn = u.i1.y != 0u;
            let wc = u.p0.zw;
            let wc_b = u.p1.x;
            let warning_col = vec3(wc.x, wc.y, wc_b);
            let out_enc = limit_video(v, max_val, comp, axis);
            let diff = abs(v - out_enc);
            if warn && (diff.x > 1e-4 || diff.y > 1e-4 || diff.z > 1e-4) {
                return vec4(warning_col * a, a);
            }
            let res = vec3(enc_to_linear_limiter(out_enc.x), enc_to_linear_limiter(out_enc.y), enc_to_linear_limiter(out_enc.z));
            return vec4(res * a, a);
        }
        case OP_LUMETRI: {
            let o = ld(p);
            let a = o.a;
            if a <= 1e-6 {
                return o;
            }
            let c = o.rgb / a;
            let res = lumetri_op(c, p);
            return vec4(res * a, a);
        }
        default: {
            let o = ld(p);
            let a = o.a;
            if a <= 1e-6 {
                return o;
            }
            return vec4(color_op(op, o.rgb / a) * a, a);
        }
    }
}

@compute @workgroup_size(16, 16)
fn fx_px(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= u.i0.y || id.y >= u.i0.z {
        return;
    }
    let p = vec2<i32>(id.xy);
    textureStore(dst, p, pixel(u.i0.x, p));
}

// A box pass as a running sum along one row (i1.z = 0) or column (1) per invocation: O(1) per
// pixel for any radius, the arithmetic of `effects::box_rows`.
@compute @workgroup_size(64)
fn fx_run(@builtin(global_invocation_id) id: vec3<u32>) {
    let s = size();
    let vertical = u.i1.z != 0u;
    let n = select(s.x, s.y, vertical);
    let lines = select(s.y, s.x, vertical);
    let line = i32(id.x);
    if line >= lines {
        return;
    }
    let r = i32(u.i1.x);
    let repeat = u.i1.y != 0u;
    let inv = 1.0 / f32(2 * r + 1);
    var acc = vec4(0.0);
    for (var i = -r; i <= r; i++) {
        if repeat || (i >= 0 && i < n) {
            let j = clamp(i, 0, n - 1);
            acc += ld(select(vec2(j, line), vec2(line, j), vertical));
        }
    }
    for (var x = 0; x < n; x++) {
        textureStore(dst, select(vec2(x, line), vec2(line, x), vertical), acc * inv);
        let out_i = x - r;
        let in_i = x + r + 1;
        if repeat || out_i >= 0 {
            let j = clamp(out_i, 0, n - 1);
            acc -= ld(select(vec2(j, line), vec2(line, j), vertical));
        }
        if repeat || in_i < n {
            let j = clamp(in_i, 0, n - 1);
            acc += ld(select(vec2(j, line), vec2(line, j), vertical));
        }
    }
}
