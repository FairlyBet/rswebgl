// Auto exposure: the frame's average brightness, reduced on the GPU and
// eased over time — ported from Godot 3.5's GLES3 renderer
// (drivers/gles3/shaders/exposure.glsl, with RasterizerSceneGLES3's
// chain: 243² → 81² → 27² → 9² → 3² → 1²; MIT, Copyright (c) 2007-2022
// Juan Linietsky, Ariel Manzur, Copyright (c) 2014-2022 Godot Engine
// contributors).
//
// Changes from the original: the BEGIN / shrink / END variants are
// EXPOSURE_BEGIN / EXPOSURE_END defines as there, FORCE_SET a uniform
// (u_force), and a forced value is clamped too (Godot keeps it as is: a
// black first frame would divide by 0); the source is premultiplied
// radiance (the HDR target), read as is; the result is written to .r of an
// RGBA16F target.

precision highp float;
precision highp int;
precision highp sampler2D;

uniform sampler2D u_source;

#ifdef EXPOSURE_BEGIN
uniform ivec2 u_source_size;
uniform ivec2 u_target_size;
#endif

#ifdef EXPOSURE_END
uniform sampler2D u_prev;    // last frame's 1×1 result
uniform float u_adjust;      // speed × seconds since it
uniform float u_min_luminance;
uniform float u_max_luminance;
uniform float u_force;       // 1: no previous to ease from
#endif

out vec4 frag_color;

void main() {
    float exposure;
#ifdef EXPOSURE_BEGIN
    ivec2 src_pos = ivec2(gl_FragCoord.xy) * u_source_size / u_target_size;
    // More precise and expensive, but less jittery (Godot's #if 1 branch).
    ivec2 next_pos = (ivec2(gl_FragCoord.xy) + ivec2(1)) * u_source_size / u_target_size;
    next_pos = max(next_pos, src_pos + ivec2(1)); // at least one pixel
    vec3 source_color = vec3(0.0);
    for (int i = src_pos.x; i < next_pos.x; i++) {
        for (int j = src_pos.y; j < next_pos.y; j++) {
            source_color += texelFetch(u_source, ivec2(i, j), 0).rgb;
        }
    }
    source_color /= float((next_pos.x - src_pos.x) * (next_pos.y - src_pos.y));
    exposure = max(source_color.r, max(source_color.g, source_color.b));
#else
    ivec2 coord = ivec2(gl_FragCoord.xy);
    exposure = texelFetch(u_source, coord * 3 + ivec2(0, 0), 0).r;
    exposure += texelFetch(u_source, coord * 3 + ivec2(1, 0), 0).r;
    exposure += texelFetch(u_source, coord * 3 + ivec2(2, 0), 0).r;
    exposure += texelFetch(u_source, coord * 3 + ivec2(0, 1), 0).r;
    exposure += texelFetch(u_source, coord * 3 + ivec2(1, 1), 0).r;
    exposure += texelFetch(u_source, coord * 3 + ivec2(2, 1), 0).r;
    exposure += texelFetch(u_source, coord * 3 + ivec2(0, 2), 0).r;
    exposure += texelFetch(u_source, coord * 3 + ivec2(1, 2), 0).r;
    exposure += texelFetch(u_source, coord * 3 + ivec2(2, 2), 0).r;
    exposure *= (1.0 / 9.0);
#ifdef EXPOSURE_END
    if (u_force < 0.5) {
        float prev_lum = texelFetch(u_prev, ivec2(0, 0), 0).r; // 1 pixel previous exposure
        exposure = clamp(prev_lum + (exposure - prev_lum) * u_adjust, u_min_luminance, u_max_luminance);
    } else {
        exposure = clamp(exposure, u_min_luminance, u_max_luminance);
    }
#endif
#endif
    frag_color = vec4(exposure, 0.0, 0.0, 1.0);
}
