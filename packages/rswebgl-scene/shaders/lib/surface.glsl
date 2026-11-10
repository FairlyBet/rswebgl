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
    vec3 transmission_tint; // OpenPBR's transmission_color at depth 0: on the surface
    float thickness;        // 0: thin-walled
    vec3 attenuation_color;
    float attenuation_distance; // 0: no absorption
    float dispersion;       // glTF: 20 / Abbe number
    float coat;
    float coat_roughness;   // perceptual
    float coat_ior;
    vec3 coat_color;        // OpenPBR coat_color (absorption)
    float coat_darkening;   // OpenPBR coat_darkening
    float coat_anisotropy;  // OpenPBR coat_roughness_anisotropy
    vec3 coat_t;            // the coat's stretch direction, and across it
    vec3 coat_b;
    float subsurface;       // OpenPBR subsurface_weight
    vec3 subsurface_color;
    vec3 subsurface_mfp;    // mean free path per channel (radius × radius_scale)
    float subsurface_radius; // curvature radius at the point
    float ao;
    vec3 emission;          // radiance
    vec3 sheen_color;       // glTF sheen; OpenPBR fuzz = its max as weight
    float sheen_roughness;
    float iridescence;      // thin-film weight
    float iridescence_ior;
    float iridescence_thickness; // nm
    float anisotropy;       // strength 0..1
    vec3 anisotropy_t;      // the stretch direction
    vec3 anisotropy_b;
    float diffuse_transmission; // KHR_materials_diffuse_transmission share
    vec3 diffuse_transmission_color;
};

// The roughness a refraction blurs with: none at IOR 1 (Khronos
// applyIorToRoughness).
float ior_roughness(float roughness, float ior) {
    return roughness * clamp(ior * 2.0 - 2.0, 0.0, 1.0);
}

// The ray through a volume of `thickness` (mesh units, scaled by the model
// matrix per axis: v_model_scale, from the vertex stage) along the
// refracted direction (Khronos getVolumeTransmissionRay).
vec3 volume_ray(vec3 n, vec3 v, float thickness, float ior) {
    vec3 r = refract(-v, n, 1.0 / ior);
    if (dot(r, r) == 0.0) r = -v; // total internal reflection (IOR < 1)
    return normalize(r) * thickness * v_model_scale;
}

// Beer's law over `distance` (Khronos applyVolumeAttenuation).
vec3 volume_attenuation(vec3 radiance, float distance, vec3 color, float at_distance) {
    if (at_distance <= 0.0 || distance <= 0.0) return radiance;
    return pow(color, vec3(distance / at_distance)) * radiance;
}

// Diffusely transmitted light through the volume: the thickness scaled by
// the model's mean scale (Khronos diffuseTransmissionThickness), straight
// through. Unchanged without a volume.
vec3 diffuse_through_volume(vec3 radiance, Surface s) {
#ifdef HAS_VOLUME
    float distance = s.thickness * (v_model_scale.x + v_model_scale.y + v_model_scale.z) / 3.0;
    return volume_attenuation(radiance, distance, s.attenuation_color, s.attenuation_distance);
#else
    return radiance;
#endif
}

// Light arriving through the surface, attenuated by the volume: met where
// the refracted view ray leaves it (straight through when thin-walled),
// each color channel bent by its own IOR with dispersion. The opaque scene
// behind it where the frame copied it (HDR output), the environment
// otherwise (Khronos getIBLVolumeRefraction / getTransmissionSample).
vec3 refracted_light(Surface s, float ior, float roughness, out float path) {
    vec3 ray = vec3(0.0);
    path = 0.0;
    if (s.thickness > 0.0) {
        ray = volume_ray(s.n, s.v, s.thickness, ior);
        path = length(ray);
    }
    float blur = ior_roughness(roughness, ior);
    if (scene_color_available()) return scene_color_at(s.position + ray, blur);
    vec3 dir = path > 0.0 ? ray / path : -s.v;
    return textureLod(u_env_specular, env_dir(dir, u_lighting.z), blur * u_lighting.w).rgb
        * u_environment.rgb;
}

vec3 env_transmission(Surface s, float ior, float roughness) {
    float path;
#ifdef HAS_DISPERSION
    // Each channel bent by its own IOR (glTF KHR_materials_dispersion).
    float half_spread = (ior - 1.0) * 0.025 * s.dispersion;
    float path_r;
    float path_b;
    vec3 c = vec3(refracted_light(s, ior - half_spread, roughness, path_r).r,
                  refracted_light(s, ior, roughness, path).g,
                  refracted_light(s, ior + half_spread, roughness, path_b).b);
#else
    vec3 c = refracted_light(s, ior, roughness, path);
#endif
    return volume_attenuation(c, path, s.attenuation_color, s.attenuation_distance);
}

// A punctual light seen through the surface: GGX around the light mirrored
// to the far side, D·V as Khronos' getPunctualRadianceTransmission returns
// it (no cosine). Without the cosine V stays above 0 for a light on the
// eye's side too, so rough glass also glows from lights in front of it —
// what the reference renderer shows (LightsPunctualLamp's shade: with a
// cosine it lost its front lights and went teal from the one behind). The
// cost: a few white sparkles where n·l and n·v both near 0 (silhouettes).
float punctual_transmission(vec3 n, vec3 v, vec3 l, float alpha, float ior) {
    float t_alpha = ior_roughness(alpha, ior);
    vec3 l_mirror = normalize(l + 2.0 * n * dot(-l, n));
    vec3 h = normalize(l_mirror + v);
    float NdotL = clamp(dot(n, l_mirror), 0.0, 1.0);
    return D_GGX(clamp(dot(n, h), 0.0, 1.0), t_alpha)
        * V_GGX(NdotL, clamp(dot(n, v), 0.0, 1.0), t_alpha);
}
