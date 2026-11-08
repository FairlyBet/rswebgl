//! The environment BRDF table, integrated on the CPU at startup. For each
//! (n·v, perceptual roughness):
//! - R, G: the split-sum scale and bias to apply to F0 for GGX specular
//!   image-based light (Karis 2013), with the height-correlated Smith term —
//!   as the Khronos glTF-Sample-Renderer's table (`ibl_filtering.frag`,
//!   Apache-2.0);
//! - B: the directional albedo of the glTF sheen BRDF (Charlie distribution,
//!   Conty & Kulla's sheen visibility, as Khronos' `brdf.glsl` evaluates it),
//!   for sheen's albedo scaling and image-based light.

/// LUT side, texels.
pub const LUT_SIZE: usize = 32;
const SAMPLES: u32 = 256;
const SHEEN_SAMPLES: u32 = 512;

/// `LUT_SIZE²` (scale, bias, sheen albedo), n·v along x (row-major),
/// roughness along y, both sampled at texel centers.
pub fn brdf_lut() -> Vec<[f32; 3]> {
    let mut out = Vec::with_capacity(LUT_SIZE * LUT_SIZE);
    for y in 0..LUT_SIZE {
        let roughness = (y as f32 + 0.5) / LUT_SIZE as f32;
        for x in 0..LUT_SIZE {
            let n_v = (x as f32 + 0.5) / LUT_SIZE as f32;
            let [scale, bias] = integrate(n_v, roughness);
            out.push([scale, bias, sheen_albedo(n_v, roughness)]);
        }
    }
    out
}

/// One (scale, bias) by GGX importance sampling.
pub fn integrate(n_v: f32, roughness: f32) -> [f32; 2] {
    let v = [(1.0 - n_v * n_v).sqrt(), 0.0, n_v];
    let a = roughness * roughness;
    let (mut scale, mut bias) = (0.0, 0.0);
    for i in 0..SAMPLES {
        let (u1, u2) = hammersley(i, SAMPLES);
        // Half vector around +Z.
        let phi = 2.0 * std::f32::consts::PI * u1;
        let cos_t = ((1.0 - u2) / (1.0 + (a * a - 1.0) * u2)).sqrt();
        let sin_t = (1.0 - cos_t * cos_t).sqrt();
        let h = [sin_t * phi.cos(), sin_t * phi.sin(), cos_t];
        let v_h = v[0] * h[0] + v[1] * h[1] + v[2] * h[2];
        let l_z = 2.0 * v_h * h[2] - v[2];
        if l_z > 0.0 {
            let n_l = l_z;
            let n_h = h[2].max(1e-6);
            // The visibility over the half-vector pdf (D n·h / 4 v·h).
            let v_pdf = 4.0 * v_smith_correlated(n_v, n_l, a) * v_h.max(0.0) * n_l / n_h;
            let fc = (1.0 - v_h.max(0.0)).powi(5);
            scale += (1.0 - fc) * v_pdf;
            bias += fc * v_pdf;
        }
    }
    [scale / SAMPLES as f32, bias / SAMPLES as f32]
}

// Height-correlated Smith visibility, G2 / (4 n·l n·v); `a` = roughness².
fn v_smith_correlated(n_v: f32, n_l: f32, a: f32) -> f32 {
    let a2 = a * a;
    let gv = n_l * (n_v * n_v * (1.0 - a2) + a2).sqrt();
    let gl = n_v * (n_l * n_l * (1.0 - a2) + a2).sqrt();
    0.5 / (gv + gl).max(1e-8)
}

/// Directional albedo of the glTF sheen BRDF (white sheen color): ∫ f n·l dω
/// by cosine-weighted sampling.
pub fn sheen_albedo(n_v: f32, roughness: f32) -> f32 {
    let v = [(1.0 - n_v * n_v).sqrt(), 0.0, n_v];
    let mut sum = 0.0;
    for i in 0..SHEEN_SAMPLES {
        let (u1, u2) = hammersley(i, SHEEN_SAMPLES);
        let phi = 2.0 * std::f32::consts::PI * u1;
        let n_l = (1.0 - u2).sqrt();
        let s = u2.sqrt();
        let l = [s * phi.cos(), s * phi.sin(), n_l];
        let h = [v[0] + l[0], v[1] + l[1], v[2] + l[2]];
        let len = (h[0] * h[0] + h[1] * h[1] + h[2] * h[2]).sqrt().max(1e-8);
        let n_h = h[2] / len;
        // pdf = n·l / π, so f n·l / pdf = π f.
        sum += std::f32::consts::PI * d_charlie(roughness, n_h) * v_sheen(n_l, n_v, roughness);
    }
    // At grazing angles on a smooth sheen the clamped visibility overshoots;
    // an albedo above 1 would turn albedo scaling negative.
    (sum / SHEEN_SAMPLES as f32).min(1.0)
}

// Khronos brdf.glsl D_Charlie (Estevez & Kulla 2017).
fn d_charlie(roughness: f32, n_h: f32) -> f32 {
    let alpha = roughness.max(1e-6).powi(2);
    let inv = 1.0 / alpha;
    let sin2 = (1.0 - n_h * n_h).max(0.0);
    (2.0 + inv) * sin2.powf(inv * 0.5) / (2.0 * std::f32::consts::PI)
}

// Khronos brdf.glsl lambdaSheen / V_Sheen.
fn lambda_sheen_helper(x: f32, alpha: f32) -> f32 {
    let t = (1.0 - alpha) * (1.0 - alpha);
    let lerp = |a: f32, b: f32| a + (b - a) * t;
    let (a, b, c, d, e) = (
        lerp(21.5473, 25.3245),
        lerp(3.82987, 3.32435),
        lerp(0.19823, 0.16801),
        lerp(-1.97760, -1.27393),
        lerp(-4.32054, -4.85967),
    );
    a / (1.0 + b * x.powf(c)) + d * x + e
}

fn lambda_sheen(cos: f32, alpha: f32) -> f32 {
    if cos.abs() < 0.5 {
        lambda_sheen_helper(cos, alpha).exp()
    } else {
        (2.0 * lambda_sheen_helper(0.5, alpha) - lambda_sheen_helper(1.0 - cos, alpha)).exp()
    }
}

fn v_sheen(n_l: f32, n_v: f32, roughness: f32) -> f32 {
    let alpha = roughness.max(1e-6).powi(2);
    let v = 1.0 / ((1.0 + lambda_sheen(n_v, alpha) + lambda_sheen(n_l, alpha)) * (4.0 * n_v * n_l));
    v.clamp(0.0, 1.0)
}

fn hammersley(i: u32, n: u32) -> (f32, f32) {
    (
        i as f32 / n as f32,
        i.reverse_bits() as f32 / 4_294_967_296.0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smooth_head_on_reflects_f0_exactly() {
        let [scale, bias] = integrate(1.0, 0.0);
        assert!(
            (scale - 1.0).abs() < 1e-3 && bias.abs() < 1e-3,
            "{scale} {bias}"
        );
    }

    #[test]
    fn energy_never_exceeds_one_and_grazing_adds_fresnel() {
        for [scale, bias, sheen] in brdf_lut() {
            assert!(scale >= 0.0 && bias >= 0.0 && scale + bias <= 1.0 + 1e-2);
            assert!((0.0..=1.0).contains(&sheen), "{sheen}");
        }
        // Grazing angles push reflectance toward white (the bias).
        let [_, head_on] = integrate(0.95, 0.3);
        let [_, grazing] = integrate(0.1, 0.3);
        assert!(grazing > head_on * 3.0, "{grazing} vs {head_on}");
        // Rough surfaces lose specular energy (no multiple scattering).
        let [s_smooth, b_smooth] = integrate(0.5, 0.1);
        let [s_rough, b_rough] = integrate(0.5, 1.0);
        assert!(s_rough + b_rough < s_smooth + b_smooth);
    }

    #[test]
    fn sheen_reflects_most_at_grazing_and_when_rough() {
        // Sheen is a retro/grazing lobe: brighter seen edge-on, and a
        // rougher (fluffier) sheen reflects more overall.
        assert!(sheen_albedo(0.1, 0.5) > sheen_albedo(0.9, 0.5));
        assert!(sheen_albedo(0.5, 1.0) > sheen_albedo(0.5, 0.2));
    }
}
