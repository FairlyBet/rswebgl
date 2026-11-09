// The display pass: the HDR target (premultiplied linear radiance and
// coverage) → the canvas: exposure, tone mapping, optional sharpening, sRGB
// encoding, and a little dither against banding in dark gradients.

precision highp float;
precision highp int;
// Samplers default to lowp, which can clip HDR values on mobile GPUs.
precision highp sampler2D;

#include "color.glsl"
#include "tonemap.glsl"

in vec2 v_ndc;
out vec4 frag_color;

uniform sampler2D u_hdr;
uniform sampler2D u_coverage; // the frame's coverage (u_hdr is TAA's history then)
uniform float u_exposure;
uniform int u_tone_mapping;
uniform float u_dither; // 0 off, 1 on
uniform float u_sharpness; // RCAS strength 0…1 (FSR1's 2^-stops), 0 off

// Interleaved gradient noise (Jimenez 2014): a per-pixel value in [0, 1).
float ign(vec2 p) {
    return fract(52.9829189 * fract(dot(p, vec2(0.06711056, 0.00583715))));
}

// The tone-mapped (display-linear) color at p, unpremultiplied.
vec3 display(ivec2 p) {
    p = clamp(p, ivec2(0), textureSize(u_hdr, 0) - 1);
    float a = texelFetch(u_coverage, p, 0).a;
    vec3 c = a > 0.0 ? texelFetch(u_hdr, p, 0).rgb / a : vec3(0.0);
    return tone_map(c * u_exposure, u_tone_mapping);
}

// Robust contrast-adaptive sharpening — ported from AMD FidelityFX FSR 1
// (ffx_fsr1.h FsrRcasF, 32-bit path, without denoise; MIT, Copyright (c)
// 2021 Advanced Micro Devices, Inc.). Sharpens by a negative lobe on the
// four neighbors, limited so no channel leaves the ring's range: no
// ringing or clipping. Exact reciprocals in place of the approximations.
#define FSR_RCAS_LIMIT (0.25 - (1.0 / 16.0))

vec3 rcas(ivec2 sp, vec3 e) {
    //    b
    //  d e f
    //    h
    vec3 b = display(sp + ivec2(0, -1));
    vec3 d = display(sp + ivec2(-1, 0));
    vec3 f = display(sp + ivec2(1, 0));
    vec3 h = display(sp + ivec2(0, 1));
    // Min and max of ring.
    vec3 mn4 = min(min(b, d), min(f, h));
    vec3 mx4 = max(max(b, d), max(f, h));
    // Immediate constants for peak range.
    vec2 peak_c = vec2(1.0, -1.0 * 4.0);
    // Limiters (denominators kept off 0: an all-black or all-white ring
    // would make 0/0 here, where FSR's rcp gives the GPU's inf).
    vec3 hit_min = min(mn4, e) / max(4.0 * mx4, 1e-6);
    vec3 hit_max = (peak_c.x - max(mx4, e)) / min(4.0 * mn4 + peak_c.y, -1e-6);
    vec3 lobe3 = max(-hit_min, hit_max);
    float lobe = max(-FSR_RCAS_LIMIT, min(max(lobe3.r, max(lobe3.g, lobe3.b)), 0.0)) * u_sharpness;
    // Resolve.
    float rcp_l = 1.0 / (4.0 * lobe + 1.0);
    return (lobe * b + lobe * d + lobe * h + lobe * f + e) * rcp_l;
}

void main() {
    ivec2 p = ivec2(gl_FragCoord.xy);
    float a = texelFetch(u_coverage, p, 0).a;
    vec3 c = display(p);
    if (u_sharpness > 0.0) {
        c = clamp(rcas(p, c), 0.0, 1.0);
    }
    c = linear_to_srgb(c);
    c += (ign(gl_FragCoord.xy) - 0.5) / 255.0 * u_dither;
    // Premultiplied again for the canvas (WebGL's default
    // premultipliedAlpha: true).
    frag_color = vec4(c * a, a);
}
