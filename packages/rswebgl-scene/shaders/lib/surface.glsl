// What a surface point is made of, gathered by the material shader and
// handed to a shading model (`surface_gltf.glsl`, `surface_openpbr.glsl`).
// Parameters are glTF's where glTF has them, plus OpenPBR's own; each model
// reads them its way. Needs common.glsl, object.glsl, environment.glsl.
//
// Transmission helpers ported from the Khronos glTF-Sample-Renderer
// (Apache-2.0), source/Renderer/shaders/punctual.glsl and functions.glsl.

struct Surface {
    vec3 position;          // world
    vec3 v;                 // toward the eye
    vec3 n;                 // shading normal (normal-mapped), facing the eye's side
    vec3 n_coat;            // the coat's normal (geometric)
    vec3 base_color;        // linear
    float opacity;          // alpha
    float base_weight;      // OpenPBR base_weight
    float metallic;
    float roughness;        // perceptual (after specular antialiasing)
    float diffuse_roughness; // OpenPBR base_diffuse_roughness (EON)
    float ior;
    float specular_weight;  // KHR_materials_specular factor / OpenPBR specular_weight
    vec3 specular_color;
    float transmission;
    float thickness;        // 0: thin-walled
    vec3 attenuation_color;
    float attenuation_distance; // 0: no absorption
    float dispersion;       // glTF: 20 / Abbe number
    float coat;
    float coat_roughness;   // perceptual
    float coat_ior;
    vec3 coat_color;        // OpenPBR coat_color (absorption)
    float coat_darkening;   // OpenPBR coat_darkening
    float ao;
    vec3 emission;          // radiance
};

// The roughness a refraction blurs with: none at IOR 1 (Khronos
// applyIorToRoughness).
float ior_roughness(float roughness, float ior) {
    return roughness * clamp(ior * 2.0 - 2.0, 0.0, 1.0);
}

// The ray through a volume of `thickness` (mesh units, scaled by the model
// matrix per axis) along the refracted direction (Khronos
// getVolumeTransmissionRay).
vec3 volume_ray(vec3 n, vec3 v, float thickness, float ior) {
    vec3 r = refract(-v, n, 1.0 / ior);
    if (dot(r, r) == 0.0) r = -v; // total internal reflection (IOR < 1)
    vec3 scale = vec3(length(u_model[0].xyz), length(u_model[1].xyz), length(u_model[2].xyz));
    return normalize(r) * thickness * scale;
}

// Beer's law over `distance` (Khronos applyVolumeAttenuation).
vec3 volume_attenuation(vec3 radiance, float distance, vec3 color, float at_distance) {
    if (at_distance <= 0.0 || distance <= 0.0) return radiance;
    return pow(color, vec3(distance / at_distance)) * radiance;
}

// Environment light refracted through the surface, attenuated by the volume:
// light enters along the refracted ray (straight through when thin-walled),
// each color channel bent by its own IOR with dispersion. The scene behind
// the surface isn't seen (the environment stands in for it).
vec3 env_refracted(Surface s, float ior, float lod, out float path) {
    vec3 dir = -s.v;
    path = 0.0;
    if (s.thickness > 0.0) {
        vec3 ray = volume_ray(s.n, s.v, s.thickness, ior);
        path = length(ray);
        dir = normalize(ray);
    }
    return textureLod(u_env_specular, env_dir(dir, u_lighting.z), lod).rgb;
}

vec3 env_transmission(Surface s, float ior, float roughness) {
    float lod = ior_roughness(roughness, ior) * u_lighting.w;
    float path;
#ifdef HAS_DISPERSION
    // Each channel bent by its own IOR (glTF KHR_materials_dispersion).
    float half_spread = (ior - 1.0) * 0.025 * s.dispersion;
    float path_r;
    float path_b;
    vec3 c = vec3(env_refracted(s, ior - half_spread, lod, path_r).r,
                  env_refracted(s, ior, lod, path).g,
                  env_refracted(s, ior + half_spread, lod, path_b).b);
#else
    vec3 c = env_refracted(s, ior, lod, path);
#endif
    c *= u_environment.rgb;
    return volume_attenuation(c, path, s.attenuation_color, s.attenuation_distance);
}

// A punctual light seen through the surface: GGX around the light mirrored
// to the far side (Khronos getPunctualRadianceTransmission).
float punctual_transmission(vec3 n, vec3 v, vec3 l, float alpha, float ior) {
    float t_alpha = ior_roughness(alpha, ior);
    vec3 l_mirror = normalize(l + 2.0 * n * dot(-l, n));
    vec3 h = normalize(l_mirror + v);
    return D_GGX(clamp(dot(n, h), 0.0, 1.0), t_alpha)
        * V_GGX(clamp(dot(n, l_mirror), 0.0, 1.0), clamp(dot(n, v), 0.0, 1.0), t_alpha);
}
