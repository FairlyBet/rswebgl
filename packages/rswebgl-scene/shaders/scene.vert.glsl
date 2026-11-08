// The scene's vertex-stage prelude: what every scene shader can use. Put in
// front of the shader (after `#version 300 es` and any defines) by
// `shader_sources`; a shader mustn't declare these itself.

precision highp float;
precision highp int;

#include "common.glsl"
#include "frame.glsl"
#include "object_instanced.glsl"
