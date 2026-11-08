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

// Linear scene radiance → the canvas: exposure, tone mapping, sRGB.
vec3 scene_output(vec3 linear) {
    return linear_to_srgb(tone_map_neutral(linear * u_lighting.y));
}
