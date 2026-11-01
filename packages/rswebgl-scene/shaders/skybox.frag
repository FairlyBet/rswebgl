// The environment as the background: the specular cube looked up along each
// pixel's view ray, at a mip for blur, through the same exposure and display
// transform as the lit scene. Drawn first, without depth, by SceneRenderer.
// `tonemap.glsl` is prepended.

precision highp sampler2D;
precision highp samplerCube;

in vec2 v_ndc;
out vec4 frag_color;

layout(std140) uniform Frame {
    mat4 u_view;
    mat4 u_proj;
    mat4 u_view_proj;
    vec4 u_camera_pos;
};
#define MAX_LIGHTS 8
struct Light {
    vec4 position;
    vec4 direction;
    vec4 color;
    vec4 params;
};
layout(std140) uniform Lighting {
    Light u_lights[MAX_LIGHTS];
    vec4 u_environment;
    vec4 u_lighting;
};
uniform samplerCube u_env_specular;
uniform float u_background_blur; // 0 sharp … 1 the roughest mip

void main() {
    // View-space ray through this pixel, then to world space (the view's
    // rotation is orthonormal: its inverse is its transpose).
    vec3 view_ray = vec3(v_ndc.x / u_proj[0][0], v_ndc.y / u_proj[1][1], -1.0);
    vec3 dir = normalize(transpose(mat3(u_view)) * view_ray);
    vec3 c = textureLod(u_env_specular, env_dir(dir, u_lighting.z),
                        u_background_blur * u_lighting.w).rgb;
    frag_color = vec4(to_srgb(tone_map(c * u_environment.rgb * u_lighting.y)), 1.0);
}
