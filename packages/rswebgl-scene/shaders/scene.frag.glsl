// The scene's fragment-stage prelude: the camera, lights and environment the
// scene binds, and helpers to light with them and to output a pixel. Prepended
// (after `#version 300 es` and any defines) by `shader_sources`; a shader
// mustn't declare these itself.

precision highp float;
precision highp int;
// Samplers default to lowp in fragment shaders, which can clip HDR values on
// mobile GPUs.
precision highp sampler2D;
precision highp samplerCube;

layout(std140) uniform Frame {
    mat4 u_view;
    mat4 u_proj;
    mat4 u_view_proj;
    vec4 u_camera_pos;
};

// Punctual lights (the lighting block, slot 0). type: 0 directional, 1 point,
// 2 spot.
#define MAX_LIGHTS 8
struct Light {
    vec4 position;  // xyz world position (point, spot)
    vec4 direction; // xyz world direction the light points (directional, spot)
    vec4 color;     // rgb color, w intensity (lux for directional, candela else)
    vec4 params;    // x range (0 = infinite), y spot scale, z spot offset, w type
};
layout(std140) uniform Lighting {
    Light u_lights[MAX_LIGHTS];
    vec4 u_environment; // rgb intensity of the environment light
    vec4 u_lighting;    // x light count, y exposure, z environment rotation,
                        // w last mip of the specular cube
};

// Environment (slot 0): diffuse irradiance (stored as E/π), specular radiance
// prefiltered by roughness (mip = roughness × u_lighting.w), and the
// split-sum BRDF table (R scale, G bias by n·v and roughness).
uniform samplerCube u_env_diffuse;
uniform samplerCube u_env_specular;
uniform sampler2D u_env_brdf;

// Object (slot 2), also visible here.
uniform mat4 u_model;
uniform mat3 u_normal_matrix;

// Display transform: Khronos PBR Neutral tone mapping, then sRGB encoding for
// an 8-bit canvas.
vec3 to_srgb(vec3 c) {
    c = clamp(c, 0.0, 1.0);
    return mix(c * 12.92, 1.055 * pow(c, vec3(1.0 / 2.4)) - 0.055, step(0.0031308, c));
}

vec3 tone_map(vec3 color) {
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

// Turns a world direction by the environment's rotation about +Y.
vec3 env_dir(vec3 d, float angle) {
    float c = cos(angle);
    float s = sin(angle);
    return vec3(c * d.x + s * d.z, d.y, -s * d.x + c * d.z);
}

// The environment's diffuse light on a surface facing world direction `n`
// (multiply by the albedo).
vec3 scene_irradiance(vec3 n) {
    return texture(u_env_diffuse, env_dir(n, u_lighting.z)).rgb * u_environment.rgb;
}

// The environment seen along world direction `r`, blurred for `roughness`.
vec3 scene_reflection(vec3 r, float roughness) {
    return textureLod(u_env_specular, env_dir(r, u_lighting.z), roughness * u_lighting.w).rgb
        * u_environment.rgb;
}

// Linear scene radiance → the canvas: exposure, tone mapping, sRGB.
vec3 scene_output(vec3 linear) {
    return to_srgb(tone_map(linear * u_lighting.y));
}
