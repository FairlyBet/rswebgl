// The scene's fragment-stage prelude: the camera, lights and environment the
// scene binds, and helpers to light with them and to output a pixel. Put in
// front of the shader (after `#version 300 es` and any defines) by
// `shader_sources`; a shader mustn't declare these itself.

precision highp float;
precision highp int;
// Samplers default to lowp in fragment shaders, which can clip HDR values on
// mobile GPUs.
precision highp sampler2D;
precision highp samplerCube;

#include "common.glsl"
#include "color.glsl"
#include "frame.glsl"
#include "lights.glsl"
#include "environment.glsl"
#include "object.glsl"
#include "tonemap.glsl"

// What a shader writes for linear scene radiance `linear`: to an HDR target,
// the radiance itself (the display pass applies exposure, tone mapping and
// sRGB); drawing straight to an 8-bit target, the displayed color.
vec3 scene_output(vec3 linear) {
    if (u_output.x < 0.5) return linear;
    return linear_to_srgb(tone_map(linear * u_lighting.y, int(u_output.y)));
}

// `scene_output` for a color meant as is (unlit materials): drawing straight
// to an 8-bit target it's only sRGB-encoded, not exposed or tone mapped. To
// an HDR target it goes through the display pass like everything else.
vec3 scene_output_unlit(vec3 linear) {
    return u_output.x < 0.5 ? linear : linear_to_srgb(linear);
}
