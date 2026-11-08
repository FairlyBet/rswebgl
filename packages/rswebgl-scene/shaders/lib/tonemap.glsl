// Tone mapping: linear scene radiance (Rec.709) → display range [0, 1],
// still linear (sRGB encoding comes after). Chosen by `tone_map(color, mode)`:
// 0 none (clamp), 1 Khronos PBR Neutral, 2 ACES filmic, 3 AgX.
//
// ACES and AgX are ported from three.js (MIT, © three.js authors,
// src/renderers/shaders/ShaderChunk/tonemapping_pars_fragment.glsl.js);
// Neutral is Khronos' reference (https://github.com/KhronosGroup/ToneMapping).

// Khronos PBR Neutral: colors below ~0.8 pass through almost unchanged,
// highlights compress toward white.
vec3 tone_map_neutral(vec3 color) {
    const float start = 0.8 - 0.04;
    const float desaturation = 0.15;
    float x = min(color.r, min(color.g, color.b));
    float offset = x < 0.08 ? x - 6.25 * x * x : 0.04;
    color -= offset;
    float peak = max(color.r, max(color.g, color.b));
    if (peak < start) return color;
    const float d = 1.0 - start;
    float new_peak = 1.0 - d * d / (peak + d - start);
    color *= new_peak / peak;
    float g = 1.0 - 1.0 / (desaturation * (peak - new_peak) + 1.0);
    return mix(color, vec3(new_peak), g);
}

// ACES filmic (Stephen Hill's RRT + ODT fit), with three.js' 1/0.6 exposure
// lift for a brighter viewing environment.
vec3 aces_rrt_odt_fit(vec3 v) {
    vec3 a = v * (v + 0.0245786) - 0.000090537;
    vec3 b = v * (0.983729 * v + 0.4329510) + 0.238081;
    return a / b;
}

vec3 tone_map_aces(vec3 color) {
    // sRGB => XYZ => D65_2_D60 => AP1 => RRT_SAT
    const mat3 input_mat = mat3(
        vec3(0.59719, 0.07600, 0.02840),
        vec3(0.35458, 0.90834, 0.13383),
        vec3(0.04823, 0.01566, 0.83777));
    // ODT_SAT => XYZ => D60_2_D65 => sRGB
    const mat3 output_mat = mat3(
        vec3(1.60475, -0.10208, -0.00327),
        vec3(-0.53108, 1.10813, -0.07276),
        vec3(-0.07367, -0.00605, 1.07602));
    color = input_mat * (color / 0.6);
    color = aces_rrt_odt_fit(color);
    return clamp(output_mat * color, 0.0, 1.0);
}

// AgX (Troy Sobotka; Filament's / Blender's form, rec.2020 working space).
vec3 agx_contrast(vec3 x) {
    vec3 x2 = x * x;
    vec3 x4 = x2 * x2;
    return 15.5 * x4 * x2 - 40.14 * x4 * x + 31.96 * x4
        - 6.868 * x2 * x + 0.4298 * x2 + 0.1191 * x - 0.00232;
}

vec3 tone_map_agx(vec3 color) {
    const mat3 srgb_to_rec2020 = mat3(
        vec3(0.6274, 0.0691, 0.0164),
        vec3(0.3293, 0.9195, 0.0880),
        vec3(0.0433, 0.0113, 0.8956));
    const mat3 rec2020_to_srgb = mat3(
        vec3(1.6605, -0.1246, -0.0182),
        vec3(-0.5876, 1.1329, -0.1006),
        vec3(-0.0728, -0.0083, 1.1187));
    const mat3 inset = mat3(
        vec3(0.856627153315983, 0.137318972929847, 0.11189821299995),
        vec3(0.0951212405381588, 0.761241990602591, 0.0767994186031903),
        vec3(0.0482516061458583, 0.101439036467562, 0.811302368396859));
    const mat3 outset = mat3(
        vec3(1.1271005818144368, -0.1413297634984383, -0.14132976349843826),
        vec3(-0.11060664309660323, 1.157823702216272, -0.11060664309660294),
        vec3(-0.016493938717834573, -0.016493938717834257, 1.2519364065950405));
    const float min_ev = -12.47393; // log2(2^-10 · 0.18)
    const float max_ev = 4.026069;  // log2(2^6.5 · 0.18)
    color = inset * (srgb_to_rec2020 * color);
    color = (log2(max(color, 1e-10)) - min_ev) / (max_ev - min_ev);
    color = agx_contrast(clamp(color, 0.0, 1.0));
    color = pow(max(outset * color, 0.0), vec3(2.2));
    return clamp(rec2020_to_srgb * color, 0.0, 1.0);
}

vec3 tone_map(vec3 color, int mode) {
    if (mode == 1) return tone_map_neutral(color);
    if (mode == 2) return tone_map_aces(color);
    if (mode == 3) return tone_map_agx(color);
    return clamp(color, 0.0, 1.0);
}
