//! The split-sum environment BRDF (Karis 2013): for each (n·v, roughness), the
//! scale and bias to apply to F0 for the specular part of image-based light.
//! Small enough to integrate on the CPU at startup.

/// LUT side, texels.
pub const LUT_SIZE: usize = 32;
const SAMPLES: u32 = 256;

/// `LUT_SIZE²` (scale, bias) pairs, n·v along x (row-major), roughness along y,
/// both sampled at texel centers.
pub fn brdf_lut() -> Vec<[f32; 2]> {
    let mut out = Vec::with_capacity(LUT_SIZE * LUT_SIZE);
    for y in 0..LUT_SIZE {
        let roughness = (y as f32 + 0.5) / LUT_SIZE as f32;
        for x in 0..LUT_SIZE {
            let n_v = (x as f32 + 0.5) / LUT_SIZE as f32;
            out.push(integrate(n_v, roughness));
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
            let n_h = h[2].max(0.0);
            let g = g_smith(n_v, n_l, roughness);
            let g_vis = g * v_h.max(0.0) / (n_h * n_v).max(1e-6);
            let fc = (1.0 - v_h.max(0.0)).powi(5);
            scale += (1.0 - fc) * g_vis;
            bias += fc * g_vis;
        }
    }
    [scale / SAMPLES as f32, bias / SAMPLES as f32]
}

// Smith G with the IBL remapping k = a / 2.
fn g_smith(n_v: f32, n_l: f32, roughness: f32) -> f32 {
    let k = roughness * roughness / 2.0;
    let g1 = |x: f32| x / (x * (1.0 - k) + k);
    g1(n_v) * g1(n_l)
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
        for [scale, bias] in brdf_lut() {
            assert!(scale >= 0.0 && bias >= 0.0 && scale + bias <= 1.0 + 1e-3);
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
}
