// The scene's vertex-stage prelude: what every scene shader can use. Prepended
// (after `#version 300 es` and any defines) by `shader_sources`; a shader
// mustn't declare these itself.

precision highp float;
precision highp int;

// Camera (the frame block, group slot 0).
layout(std140) uniform Frame {
    mat4 u_view;
    mat4 u_proj;
    mat4 u_view_proj;
    vec4 u_camera_pos; // xyz world position of the eye
};

// Object (group slot 2): local → world, and the inverse-transpose for
// normals.
uniform mat4 u_model;
uniform mat3 u_normal_matrix;
