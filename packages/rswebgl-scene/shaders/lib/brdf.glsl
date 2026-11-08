// Microfacet pieces shared by the glTF and OpenPBR models: GGX, Smith
// visibility, Fresnel (Schlick, exact dielectric, F82-tint), directional
// albedo and multiple-scattering compensation. Needs common.glsl.
//
// Ported from (Apache-2.0):
// - Khronos glTF-Sample-Renderer, source/Renderer/shaders/brdf.glsl
//   (D_GGX, V_GGX, F_Schlick);
// - MaterialX, libraries/pbrlib/genglsl/lib/mx_microfacet_specular.glsl
//   (mx_fresnel_dielectric, mx_fresnel_hoffman_schlick,
//   mx_ggx_dir_albedo_analytic, mx_ggx_energy_compensation).

// Trowbridge-Reitz / GGX normal distribution; `alpha` = roughness², floored
// (as Filament does) so a mirror-smooth surface keeps a finite peak instead
// of 0/0 or Inf in fp32 where n·h = 1.
float D_GGX(float NdotH, float alpha) {
    float a2 = sq(max(alpha, 0.002));
    float f = (NdotH * NdotH) * (a2 - 1.0) + 1.0;
    return a2 / (PI * f * f);
}

// Height-correlated Smith visibility, G2 / (4 NdotL NdotV) (Heitz 2014).
float V_GGX(float NdotL, float NdotV, float alpha) {
    float a2 = alpha * alpha;
    float ggx_v = NdotL * sqrt(NdotV * NdotV * (1.0 - a2) + a2);
    float ggx_l = NdotV * sqrt(NdotL * NdotL * (1.0 - a2) + a2);
    float ggx = ggx_v + ggx_l;
    return ggx > 0.0 ? 0.5 / ggx : 0.0;
}

vec3 F_Schlick(vec3 f0, vec3 f90, float VdotH) {
    return f0 + (f90 - f0) * pow5(clamp(1.0 - VdotH, 0.0, 1.0));
}

float F_Schlick(float f0, float f90, float VdotH) {
    return f0 + (f90 - f0) * pow5(clamp(1.0 - VdotH, 0.0, 1.0));
}

// Normal-incidence reflectance of a dielectric of relative IOR `ior`.
float ior_to_f0(float ior) {
    return sq((ior - 1.0) / (ior + 1.0));
}

// Exact unpolarized Fresnel reflectance of a dielectric interface, relative
// IOR `eta` (1 on total internal reflection). Lagarde 2013, "Memo on Fresnel
// equations".
float fresnel_dielectric(float c, float eta) {
    float g2 = eta * eta + c * c - 1.0;
    if (g2 < 0.0) return 1.0;
    float g = sqrt(g2);
    return 0.5 * sq((g - c) / (g + c)) * (1.0 + sq(((g + c) * c - 1.0) / ((g - c) * c + 1.0)));
}

// F82-tint metal Fresnel (Hoffman 2019, "Generalization of Adobe's Fresnel
// Model"): Schlick from `f0` to white, pulled toward `f82` × Schlick at
// cos = 1/7. Clamped at 0 (the correction can undershoot).
vec3 fresnel_f82(float mu, vec3 f0, vec3 f82) {
    const float COS_MAX = 1.0 / 7.0;
    const float FACTOR = 17.6513846; // 1 / (COS_MAX · (1 − COS_MAX)^6)
    float x = clamp(mu, 0.0, 1.0);
    vec3 schlick_max = mix(f0, vec3(1.0), 0.46266437); // (1 − 1/7)^5
    vec3 a = schlick_max * (vec3(1.0) - f82) * FACTOR;
    float x1 = 1.0 - x;
    float x1_2 = x1 * x1;
    return max(mix(f0, vec3(1.0), pow5(x1)) - a * x * x1_2 * x1_2 * x1_2, 0.0);
}

// GGX directional albedo split into (F0, F90) weights: a rational fit to
// Monte Carlo data (MaterialX mx_ggx_dir_albedo_analytic).
vec2 ggx_dir_albedo_ab(float NdotV, float alpha) {
    float x = NdotV;
    float y = alpha;
    float x2 = x * x;
    float y2 = y * y;
    vec4 r = vec4(0.1003, 0.9345, 1.0, 1.0) +
             vec4(-0.6303, -2.323, -1.765, 0.2281) * x +
             vec4(9.748, 2.229, 8.263, 15.94) * y +
             vec4(-2.038, -3.748, 11.53, -55.83) * x * y +
             vec4(29.34, 1.424, 28.96, 13.08) * x2 +
             vec4(-8.245, -0.7684, -7.507, 41.26) * y2 +
             vec4(-26.44, 1.436, -36.11, 54.9) * x2 * y +
             vec4(19.99, 0.2913, 15.86, 300.2) * x * y2 +
             vec4(-5.448, 0.6286, 33.37, -285.1) * x2 * y2;
    return clamp(r.xy / r.zw, 0.0, 1.0);
}

vec3 ggx_dir_albedo(float NdotV, float alpha, vec3 f0, vec3 f90) {
    vec2 ab = ggx_dir_albedo_ab(NdotV, alpha);
    return f0 * ab.x + f90 * ab.y;
}

// Multiple-scattering energy compensation (Turquin 2019, eqs. 14 and 16):
// scales single-scattering GGX so rough surfaces don't lose energy.
vec3 ggx_energy_compensation(float NdotV, float alpha, vec3 f0, vec3 f90) {
    vec2 ab = ggx_dir_albedo_ab(NdotV, alpha);
    float e_ss = max(ab.x + ab.y, 1e-4);
    vec3 f_avg = f0 + (f90 - f0) * (1.0 / 21.0);
    return 1.0 + f_avg * (1.0 - e_ss) / e_ss;
}

// Specular antialiasing (Kaplanyan 2016; Tokuyoshi & Kaplanyan 2019, as in
// Filament): widens perceptual roughness where the normal varies within the
// pixel, so thin highlights don't sparkle and alias. `variance` is the
// screen-space filter's (Filament's default 0.15; 0 turns it off),
// `threshold` caps the added α² (0.2).
float specular_aa(vec3 n, float perceptual_roughness, float variance, float threshold) {
    vec3 du = dFdx(n);
    vec3 dv = dFdy(n);
    float kernel = min(2.0 * variance * (dot(du, du) + dot(dv, dv)), threshold);
    float alpha = perceptual_roughness * perceptual_roughness;
    return sqrt(sqrt(clamp(alpha * alpha + kernel, 0.0, 1.0)));
}

// Specular occlusion from ambient occlusion (Lagarde & de Rousiers 2014,
// "Moving Frostbite to PBR"; Filament's SpecularAO_Lagarde).
float specular_occlusion(float NdotV, float ao, float alpha) {
    return clamp(pow(NdotV + ao, exp2(-16.0 * alpha - 1.0)) - 1.0 + ao, 0.0, 1.0);
}

// Anisotropic GGX (Burley 2012 addendum; Khronos D_GGX_anisotropic): `at`
// along the anisotropy tangent, `ab` along the bitangent.
float D_GGX_aniso(float NdotH, float TdotH, float BdotH, float at, float ab) {
    at = max(at, 0.002);
    ab = max(ab, 0.002);
    float a2 = at * ab;
    vec3 f = vec3(ab * TdotH, at * BdotH, a2 * NdotH);
    float w2 = a2 / dot(f, f);
    return a2 * w2 * w2 * INV_PI;
}

// Anisotropic height-correlated Smith visibility (Heitz 2014; Khronos
// V_GGX_anisotropic).
float V_GGX_aniso(float NdotL, float NdotV, float BdotV, float TdotV, float TdotL, float BdotL,
                  float at, float ab) {
    float ggx_v = NdotL * length(vec3(at * TdotV, ab * BdotV, NdotV));
    float ggx_l = NdotV * length(vec3(at * TdotL, ab * BdotL, NdotL));
    return clamp(0.5 / max(ggx_v + ggx_l, 1e-8), 0.0, 1.0);
}

// The reflection direction for image-based light on an anisotropic
// surface: reflected about a normal bent toward the grooves, the more the
// stronger and smoother (Khronos getIBLRadianceAnisotropy, after
// McAuley's / Filament's bent-normal approximation).
vec3 aniso_reflection(vec3 n, vec3 v, float roughness, float anisotropy, vec3 dir_b) {
    vec3 aniso_t = cross(dir_b, v);
    vec3 aniso_n = cross(aniso_t, dir_b);
    float bend = 1.0 - anisotropy * (1.0 - roughness);
    vec3 bent = normalize(mix(aniso_n, n, bend * bend * bend * bend));
    return reflect(-v, bent);
}
