// Cloth-like retro-reflection on top of a surface: glTF's sheen (Charlie)
// and OpenPBR's fuzz (Zeltner's LTC sheen). Needs common.glsl,
// environment.glsl (u_env_brdf: B = the glTF sheen's directional albedo).
//
// Ported from (Apache-2.0):
// - Khronos glTF-Sample-Renderer, brdf.glsl (D_Charlie, lambdaSheen,
//   V_Sheen) and pbr.frag (albedo scaling);
// - MaterialX, libraries/pbrlib/genglsl/lib/mx_microfacet_sheen.glsl
//   (mx_zeltner_sheen_dir_albedo, _ltc_aInv, _ltc_bInv,
//   mx_zeltner_sheen_brdf), after github.com/tizian/ltc-sheen — Zeltner,
//   Burley, Chiang 2022, "Practical Multiple-Scattering Sheen Using Linearly
//   Transformed Cosines".

// --- glTF sheen -------------------------------------------------------------

// Estevez & Kulla 2017; `roughness` perceptual (α = roughness²).
float D_Charlie(float roughness, float NdotH) {
    float alpha = max(roughness, 1e-6);
    alpha *= alpha;
    float inv = 1.0 / alpha;
    float sin2 = 1.0 - NdotH * NdotH;
    return (2.0 + inv) * pow(max(sin2, 0.0), inv * 0.5) / (2.0 * PI);
}

float lambda_sheen_helper(float x, float alpha) {
    float t = sq(1.0 - alpha);
    float a = mix(21.5473, 25.3245, t);
    float b = mix(3.82987, 3.32435, t);
    float c = mix(0.19823, 0.16801, t);
    float d = mix(-1.97760, -1.27393, t);
    float e = mix(-4.32054, -4.85967, t);
    return a / (1.0 + b * pow(x, c)) + d * x + e;
}

float lambda_sheen(float cos_theta, float alpha) {
    if (abs(cos_theta) < 0.5) return exp(lambda_sheen_helper(cos_theta, alpha));
    return exp(2.0 * lambda_sheen_helper(0.5, alpha) - lambda_sheen_helper(1.0 - cos_theta, alpha));
}

float V_Sheen(float NdotL, float NdotV, float roughness) {
    float alpha = sq(max(roughness, 1e-6));
    return clamp(1.0 / ((1.0 + lambda_sheen(NdotV, alpha) + lambda_sheen(NdotL, alpha))
                        * (4.0 * NdotV * NdotL)), 0.0, 1.0);
}

// The glTF sheen's directional albedo (white color), from the table.
float gltf_sheen_albedo(float NdotV, float roughness) {
    return texture(u_env_brdf, clamp(vec2(NdotV, roughness), 0.0, 1.0)).b;
}

// --- OpenPBR fuzz (Zeltner LTC sheen) ----------------------------------------

// Directional albedo: a Gaussian fit to the reference table.
float fuzz_albedo(float NdotV, float roughness) {
    float x = NdotV;
    float y = clamp(roughness, 0.01, 1.0);
    float s = y * (0.0206607 + 1.58491 * y) / (0.0379424 + y * (1.32227 + y));
    float m = y * (-0.193854 + y * (-1.14885 + y * (1.7932 - 0.95943 * y * y))) / (0.046391 + y);
    float o = y * (0.000654023 + (-0.0207818 + 0.119681 * y) * y) / (1.26264 + y * (-1.92021 + y));
    return exp(-0.5 * sq((x - m) / s)) / (s * sqrt(2.0 * PI)) + o;
}

// The lobe's shape: the LTC distribution, already including the cosine
// (multiply by the albedo, not by n·l).
float fuzz_lobe(vec3 l, vec3 v, vec3 n, float NdotV, float roughness) {
    float y = clamp(roughness, 0.01, 1.0);
    vec3 x_axis = v - n * NdotV;
    float len2 = dot(x_axis, x_axis);
    // Seen head-on any tangent will do.
    x_axis = len2 > 1e-8 ? x_axis * inversesqrt(len2)
        : normalize(abs(n.z) < 0.999 ? cross(n, vec3(0.0, 0.0, 1.0)) : cross(n, vec3(1.0, 0.0, 0.0)));
    vec3 y_axis = cross(n, x_axis);
    vec3 w = vec3(dot(l, x_axis), dot(l, y_axis), dot(l, n));
    float a_inv = (2.58126 * NdotV + 0.813703 * y) * y
        / (1.0 + 0.310327 * NdotV * NdotV + 2.60994 * NdotV * y);
    float b_inv = sqrt(1.0 - NdotV) * (y - 1.0) * y * y * y
        / (0.0000254053 + 1.71228 * NdotV - 1.71506 * NdotV * y + 1.34174 * y * y);
    vec3 wo = vec3(a_inv * w.x + b_inv * w.z, a_inv * w.y, w.z);
    float len_sq = dot(wo, wo);
    return max(wo.z, 0.0) * INV_PI * sq(a_inv / len_sq);
}
