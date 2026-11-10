// Thin-film interference (Belcour & Barla 2017, "A Practical Extension to
// Microfacet Theory for the Modeling of Varying Iridescence"): the Fresnel
// of a base under a thin transparent film, as colors shift with thickness
// and angle. Needs common.glsl, brdf.glsl.
//
// Two ports (Apache-2.0):
// - glTF: Khronos glTF-Sample-Renderer iridescence.glsl (evalIridescence:
//   Schlick interfaces, unpolarized, sensitivity to Rec.709);
// - OpenPBR: MaterialX mx_microfacet_specular.glsl (mx_fresnel_airy:
//   polarized exact dielectric first interface, generalized-Schlick base,
//   XYZ → CIE RGB), but a metal base as glTF takes it (an IOR from its F0,
//   Schlick: KHR_materials_iridescence, what's stored), not F82.

// The spectral sensitivity of the eye to an optical path difference, in
// Fourier space (Gaussian fits): XYZ, `opd_nm` in nanometres.
vec3 iridescence_sensitivity_xyz(float opd_nm, vec3 shift) {
    float phase = 2.0 * PI * opd_nm * 1.0e-9;
    vec3 val = vec3(5.4856e-13, 4.4201e-13, 5.2481e-13);
    vec3 pos = vec3(1.6810e+06, 1.7953e+06, 2.2084e+06);
    vec3 var = vec3(4.3278e+09, 9.3046e+09, 6.6121e+09);
    vec3 xyz = val * sqrt(2.0 * PI * var) * cos(pos * phase + shift) * exp(-sq(phase) * var);
    xyz.x += 9.7470e-14 * sqrt(2.0 * PI * 4.5282e+09) * cos(2.2399e+06 * phase + shift[0])
        * exp(-4.5282e+09 * sq(phase));
    return xyz / 1.0685e-7;
}

vec3 f0_to_ior(vec3 f0) {
    vec3 s = sqrt(clamp(f0, 0.0, 0.9999));
    return (vec3(1.0) + s) / (vec3(1.0) - s);
}

vec3 ior_to_f0_rel(vec3 transmitted, float incident) {
    return sq((transmitted - vec3(incident)) / (transmitted + vec3(incident)));
}

// --- glTF (Khronos evalIridescence) ------------------------------------------

vec3 gltf_iridescence(float outside_ior, float film_ior, float cos1, float thickness_nm,
                      vec3 base_f0) {
    const mat3 XYZ_TO_REC709 = mat3(
        3.2404542, -0.9692660, 0.0556434,
        -1.5371385, 1.8760108, -0.2040259,
        -0.4985314, 0.0415560, 1.0572252);
    // The film's IOR fades to the outside's as it gets very thin.
    float ior = mix(outside_ior, film_ior, smoothstep(0.0, 0.03, thickness_nm));
    float sin2_sq = sq(outside_ior / ior) * (1.0 - sq(cos1));
    float cos2_sq = 1.0 - sin2_sq;
    if (cos2_sq < 0.0) return vec3(1.0); // total internal reflection
    float cos2 = sqrt(cos2_sq);

    // First interface.
    float r0 = sq((ior - outside_ior) / (ior + outside_ior));
    float r12 = F_Schlick(r0, 1.0, cos1);
    float t121 = 1.0 - r12;
    float phi12 = ior < outside_ior ? PI : 0.0;
    float phi21 = PI - phi12;

    // Second interface.
    vec3 base_ior = f0_to_ior(base_f0);
    vec3 r1 = ior_to_f0_rel(base_ior, ior);
    vec3 r23 = F_Schlick(r1, vec3(1.0), cos2);
    vec3 phi23 = vec3(base_ior.r < ior ? PI : 0.0, base_ior.g < ior ? PI : 0.0,
                      base_ior.b < ior ? PI : 0.0);

    float opd = 2.0 * ior * thickness_nm * cos2;
    vec3 phi = vec3(phi21) + phi23;
    vec3 r123 = clamp(r12 * r23, 1e-5, 0.9999);
    vec3 r123_sqrt = sqrt(r123);
    vec3 rs = sq(t121) * r23 / (vec3(1.0) - r123);
    vec3 i = r12 + rs;
    vec3 cm = rs - t121;
    for (int m = 1; m <= 2; ++m) {
        cm *= r123_sqrt;
        i += cm * 2.0 * (XYZ_TO_REC709 * iridescence_sensitivity_xyz(float(m) * opd, float(m) * phi));
    }
    return max(i, vec3(0.0));
}

// Mixes `a` toward `b` per channel by `t` (Khronos rgb_mix: the base under
// a colored Fresnel keeps what that channel doesn't reflect).
vec3 rgb_mix(vec3 a, vec3 b, vec3 t) {
    return (1.0 - max3(t)) * a + t * b;
}

// --- OpenPBR (MaterialX mx_fresnel_airy) -------------------------------------

// Unpolarized-pair exact dielectric reflectance (p, s).
vec2 fresnel_dielectric_polarized(float cos_theta, float ior) {
    float c2 = sq(clamp(cos_theta, 0.0, 1.0));
    float s2 = 1.0 - c2;
    float t0 = max(ior * ior - s2, 0.0);
    float t1 = t0 + c2;
    float t2 = 2.0 * sqrt(t0) * cos_theta;
    float rs = (t1 - t2) / max(t1 + t2, 1e-8);
    float t3 = c2 * t0 + s2 * s2;
    float t4 = t2 * s2;
    float rp = rs * (t3 - t4) / max(t3 + t4, 1e-8);
    return vec2(rp, rs);
}

// The film over a base whose own Fresnel is `base_fresnel(cos)` — given
// as its normal-incidence F0 and F82 tint (metal) or a dielectric IOR —
// in air. `thickness_nm` in nanometres.
vec3 openpbr_thin_film(float cos_theta, float film_ior, float thickness_nm,
                       vec3 base_f0, vec3 base_f82, bool dielectric_base, float base_eta) {
    const mat3 XYZ_TO_RGB = mat3(2.3706743, -0.5138850, 0.0052982,
                                 -0.9000405, 1.4253036, -0.0146949,
                                 -0.4706338, 0.0885814, 1.0093968);
    float eta2 = max(film_ior, 1.0);
    vec3 eta3 = dielectric_base ? vec3(base_eta) : f0_to_ior(clamp(base_f0, 0.01, 0.99));
    float cos_t = sqrt(max(0.0, 1.0 - (1.0 - sq(cos_theta)) / sq(eta2)));

    vec2 r12 = fresnel_dielectric_polarized(cos_theta, eta2);
    if (cos_t <= 0.0) r12 = vec2(1.0);
    vec2 t121 = vec2(1.0) - r12;

    // The base interface, seen from inside the film.
    // A metal as glTF's KHR_materials_iridescence takes it (what's stored,
    // the Khronos renderer's evalIridescence): an IOR from its F0, Schlick
    // against the film — MaterialX's F82 conductor left a dark metal (F0 ≈
    // 0: CompareIridescence's black sphere) with nothing to interfere with.
    vec3 f23 = dielectric_base
        ? vec3(fresnel_dielectric(cos_t, base_eta / eta2))
        : F_Schlick(ior_to_f0_rel(eta3, eta2), vec3(1.0), cos_t);
    // Unpolarized base reflectance split equally: Rp = Rs = F. (MaterialX
    // halves it on its Schlick branch, which loses half of a metal under a
    // film; the spec wants the film non-absorbing.)
    vec3 r23p = f23;
    vec3 r23s = f23;

    float cos_b = cos(atan(eta2));
    vec2 phi21 = vec2(cos_theta < cos_b ? 0.0 : PI, PI);
    vec3 phi23 = vec3(eta3.r < eta2 ? PI : 0.0, eta3.g < eta2 ? PI : 0.0, eta3.b < eta2 ? PI : 0.0);
    vec3 r123p = max(sqrt(r12.x * r23p), 0.0);
    vec3 r123s = max(sqrt(r12.y * r23s), 0.0);

    float opd = 2.0 * eta2 * cos_t * thickness_nm;
    vec3 i = vec3(0.0);
    vec3 rs_p = sq(t121.x) * r23p / (vec3(1.0) - r12.x * r23p);
    i += r12.x + rs_p;
    vec3 cm = rs_p - t121.x;
    for (int m = 1; m <= 2; m++) {
        cm *= r123p;
        i += cm * 2.0 * iridescence_sensitivity_xyz(float(m) * opd, float(m) * (phi23 + vec3(phi21.x)));
    }
    vec3 rs_s = sq(t121.y) * r23s / (vec3(1.0) - r12.y * r23s);
    i += r12.y + rs_s;
    cm = rs_s - t121.y;
    for (int m = 1; m <= 2; m++) {
        cm *= r123s;
        i += cm * 2.0 * iridescence_sensitivity_xyz(float(m) * opd, float(m) * (phi23 + vec3(phi21.y)));
    }
    return clamp(XYZ_TO_RGB * (i * 0.5), 0.0, 1.0);
}
