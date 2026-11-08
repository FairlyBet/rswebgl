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

// The opaque scene behind transmissive surfaces, copied with mips after the
// opaque draws (u_output.z = 1 when there is one this frame, u_output.w its
// last mip). Needs frame.glsl.
uniform sampler2D u_scene_color;

// Depth peeling (the frame sets it; u_shading.z the mode, w 1 under
// reverse-Z): mode 1 keeps only what lies beyond the layer in u_peel_depth
// (peeling the next one), mode 2 only what lies on it (shading that layer).
uniform highp sampler2D u_peel_depth;

void peel_discard() {
    float mode = u_shading.z;
    if (mode < 0.5) return;
    float layer = texelFetch(u_peel_depth, ivec2(gl_FragCoord.xy), 0).r;
    // > 0: farther than the layer. Without reverse-Z depth is 24-bit (a step
    // is 2^-24: a few of them); with it, 32F, whose steps shrink with the
    // depth: relative, or far glass a centimeter apart would merge.
    bool reverse = u_shading.w > 0.5;
    float beyond = reverse ? layer - gl_FragCoord.z : gl_FragCoord.z - layer;
    float EPS = reverse ? max(abs(layer) * 1e-5, 1e-10) : 4e-7;
    if (mode < 1.5 ? beyond <= EPS : abs(beyond) > EPS) discard;
}

bool scene_color_available() {
    return u_output.z > 0.5;
}

// The opaque scene seen at world point `p`, blurred for `roughness`
// (perceptual; 1 → the last mip).
vec3 scene_color_at(vec3 p, float roughness) {
    vec4 clip = u_view_proj * vec4(p, 1.0);
    vec2 uv = clip.xy / clip.w * 0.5 + 0.5;
    return textureLod(u_scene_color, uv, roughness * u_output.w).rgb;
}
