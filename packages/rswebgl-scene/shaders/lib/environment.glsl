// The environment (slot 0): diffuse irradiance (stored as E/π), specular
// radiance prefiltered by roughness (mip = roughness × u_lighting.w), and
// the split-sum BRDF table (R scale, G bias by n·v and roughness). Needs
// lights.glsl (u_environment, u_lighting).

uniform samplerCube u_env_diffuse;
uniform samplerCube u_env_specular;
uniform sampler2D u_env_brdf;

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

// The environment seen along world direction `r`, blurred for `roughness`
// (perceptual).
vec3 scene_reflection(vec3 r, float roughness) {
    return textureLod(u_env_specular, env_dir(r, u_lighting.z), roughness * u_lighting.w).rgb
        * u_environment.rgb;
}
