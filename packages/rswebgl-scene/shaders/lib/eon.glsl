// EON: energy-preserving Oren-Nayar diffuse (Portsmouth, Kutz, Hill 2024),
// OpenPBR's base diffuse. At roughness 0 it is Lambert. Needs common.glsl.
//
// Ported from MaterialX (Apache-2.0), libraries/pbrlib/genglsl/lib/
// mx_microfacet_diffuse.glsl: mx_oren_nayar_compensated_diffuse and its
// albedos (Fujii's closed form). Unlike MaterialX, L·V isn't clamped to ≥ 0:
// the OpenPBR spec defines s with the raw ω_i·ω_o.

const float EON_C1 = 0.5 - 2.0 / (3.0 * PI);
const float EON_C2 = 2.0 / 3.0 - 28.0 / (15.0 * PI);

// Directional albedo of the single-scattering (FON) lobe.
float eon_albedo(float mu, float sigma) {
    float a = 1.0 / (1.0 + EON_C1 * sigma);
    float b = sigma * a;
    float c = clamp(mu, 1e-4, 1.0);
    float s = sqrt(max(0.0, 1.0 - c * c));
    float g = s * (acos(c) - s * c) + (2.0 / 3.0) * ((s / c) * (1.0 - s * s * s) - s);
    return a + b * INV_PI * g;
}

float eon_avg_albedo(float sigma) {
    return (1.0 + EON_C2 * sigma) / (1.0 + EON_C1 * sigma);
}

// The multiple-scattering color: ρ² ⟨E⟩ / (1 − ρ(1 − ⟨E⟩)).
vec3 eon_ms_color(vec3 rho, float avg) {
    return rho * rho * avg / (vec3(1.0) - rho * max(0.0, 1.0 - avg));
}

// The BRDF (including 1/π, not the cosine) for color `rho`, roughness
// `sigma`.
vec3 eon_brdf(vec3 rho, float sigma, float NdotL, float NdotV, float LdotV) {
    float s = LdotV - NdotL * NdotV;
    float stinv = s > 0.0 ? s / max(NdotL, NdotV) : s;
    float a = 1.0 / (1.0 + EON_C1 * sigma);
    vec3 single_scatter = rho * a * (1.0 + sigma * stinv);
    float e_v = eon_albedo(NdotV, sigma);
    float e_l = eon_albedo(NdotL, sigma);
    float avg = eon_avg_albedo(sigma);
    vec3 multi_scatter = eon_ms_color(rho, avg)
        * max(1e-4, 1.0 - e_v) * max(1e-4, 1.0 - e_l) / max(1e-4, 1.0 - avg);
    return (single_scatter + multi_scatter) * INV_PI;
}

// Directional albedo for the view direction, for image-based light
// (irradiance × this): mix(ρ_ms, ρ, Ê(μ_o)).
vec3 eon_dir_albedo(vec3 rho, float sigma, float NdotV) {
    return mix(eon_ms_color(rho, eon_avg_albedo(sigma)), rho, eon_albedo(NdotV, sigma));
}

// The base diffuse as the material variant has it: EON with
// HAS_DIFFUSE_ROUGHNESS, else Lambert (what EON is at roughness 0, without
// its cost).
vec3 diffuse_brdf(vec3 rho, float sigma, float NdotL, float NdotV, float LdotV) {
#ifdef HAS_DIFFUSE_ROUGHNESS
    return eon_brdf(rho, sigma, NdotL, NdotV, LdotV);
#else
    return rho * INV_PI;
#endif
}

vec3 diffuse_albedo(vec3 rho, float sigma, float NdotV) {
#ifdef HAS_DIFFUSE_ROUGHNESS
    return eon_dir_albedo(rho, sigma, NdotV);
#else
    return rho;
#endif
}
