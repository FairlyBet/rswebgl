// The glTF 2.0 reference shading model: what the Khronos sample renderer
// computes, for comparing against OpenPBR on the same material. Base
// metallic-roughness (glTF spec appendix B), KHR_materials_ior, _specular,
// _clearcoat, _sheen, _iridescence, _anisotropy, _transmission, _volume,
// _dispersion, _emissive_strength. Needs brdf.glsl, sheen.glsl,
// iridescence.glsl, surface.glsl, lights.glsl, environment.glsl.
//
// Ported from the Khronos glTF-Sample-Renderer (Apache-2.0),
// source/Renderer/shaders/pbr.frag, ibl.glsl, punctual.glsl,
// material_info.glsl. Differences:
// - transmission sees a half-size copy of the opaque scene with HDR output
//   (as Khronos' transmission framebuffer), the environment otherwise;
// - sheen's image-based light reads the GGX-prefiltered environment at the
//   sheen roughness (no Charlie-prefiltered map), scaled by the sheen
//   albedo the BRDF table integrates from the same punctual BRDF.

// Roughness-dependent Fresnel for image-based light, with multiple
// scattering (Fdez-Agüera 2019; Khronos getIBLGGXFresnel).
vec3 gltf_ibl_fresnel(float NdotV, float roughness, vec3 f0, float specular_weight) {
    vec2 f_ab = texture(u_env_brdf, clamp(vec2(NdotV, roughness), 0.0, 1.0)).rg;
    vec3 fr = max(vec3(1.0 - roughness), f0) - f0;
    vec3 k_s = f0 + fr * pow5(1.0 - NdotV);
    vec3 fss_ess = specular_weight * (k_s * f_ab.x + f_ab.y);
    float ems = 1.0 - (f_ab.x + f_ab.y);
    vec3 f_avg = specular_weight * (f0 + (1.0 - f0) / 21.0);
    vec3 fms_ems = ems * fss_ess * f_avg / (1.0 - f_avg * ems);
    return fss_ess + fms_ems;
}

vec3 shade_gltf(Surface s) {
    vec3 n = s.n;
    vec3 v = s.v;
    vec3 base = s.base_color;
    float NdotV = clamp(dot(n, v), 0.0, 1.0);
    float roughness = s.roughness;
    float alpha = roughness * roughness;

    // KHR_materials_ior, _specular.
    vec3 f0_dielectric = min(vec3(ior_to_f0(s.ior)) * s.specular_color, vec3(1.0));
    float specular_weight = s.specular_weight;
    vec3 f90_dielectric = vec3(specular_weight);

    float clearcoat = 0.0;
    float clearcoat_fresnel = 0.0;
#ifdef HAS_CLEARCOAT
    clearcoat = s.coat;
    // As the reference renderer does: the coat's F0 follows the material IOR.
    float coat_f0 = ior_to_f0(s.ior);
    float coat_alpha = s.coat_roughness * s.coat_roughness;
    clearcoat_fresnel = F_Schlick(coat_f0, 1.0, clamp(dot(s.n_coat, v), 0.0, 1.0));
#endif

#ifdef HAS_IRIDESCENCE
    // The film's Fresnel over each base, seen along the view.
    float iridescence = s.iridescence_thickness > 0.0 ? s.iridescence : 0.0;
    // Only over the bases present (the film is costly): mixed in by weights
    // that are zero otherwise.
    vec3 irid_dielectric = vec3(0.0);
    vec3 irid_metal = vec3(0.0);
    if (iridescence > 0.0 && s.metallic < 1.0)
        irid_dielectric = gltf_iridescence(1.0, s.iridescence_ior, NdotV,
                                           s.iridescence_thickness, f0_dielectric);
    if (iridescence > 0.0 && s.metallic > 0.0)
        irid_metal = gltf_iridescence(1.0, s.iridescence_ior, NdotV,
                                      s.iridescence_thickness, base);
#endif

#ifdef HAS_SHEEN
    float sheen_max = max3(s.sheen_color);
    float sheen_scaling = 1.0 - sheen_max * gltf_sheen_albedo(NdotV, s.sheen_roughness);
#endif

    // Image-based light.
    vec3 f_diffuse = scene_irradiance(n) * base;
#ifdef HAS_DIFFUSE_TRANSMISSION
    // KHR_materials_diffuse_transmission (Khronos sample renderer): a share
    // of the diffuse is the light from the other side, by its own color,
    // through the volume's thickness.
    vec3 dt_ibl = diffuse_through_volume(scene_irradiance(-n) * s.diffuse_transmission_color, s);
    f_diffuse = mix(f_diffuse, dt_ibl, s.diffuse_transmission);
#endif
#ifdef HAS_TRANSMISSION
    vec3 f_transmission = env_transmission(s, s.ior, roughness) * base;
    f_diffuse = mix(f_diffuse, f_transmission, s.transmission);
#endif
#ifdef HAS_ANISOTROPY
    vec3 f_specular = scene_reflection(
        aniso_reflection(n, v, roughness, s.anisotropy, s.anisotropy_b), roughness);
#else
    vec3 f_specular = scene_reflection(reflect(-v, n), roughness);
#endif
    vec3 metal_ibl = gltf_ibl_fresnel(NdotV, roughness, base, 1.0) * f_specular;
    vec3 dielectric_fresnel_ibl = gltf_ibl_fresnel(NdotV, roughness, f0_dielectric, specular_weight);
    vec3 dielectric_ibl = mix(f_diffuse, f_specular, dielectric_fresnel_ibl);
#ifdef HAS_IRIDESCENCE
    metal_ibl = mix(metal_ibl, f_specular * irid_metal, iridescence);
    dielectric_ibl = mix(dielectric_ibl, rgb_mix(f_diffuse, f_specular, irid_dielectric),
                         iridescence);
#endif
    vec3 color = mix(dielectric_ibl, metal_ibl, s.metallic);
#ifdef HAS_SHEEN
    vec3 sheen_ibl = scene_reflection(reflect(-v, n), s.sheen_roughness) * s.sheen_color
        * gltf_sheen_albedo(NdotV, s.sheen_roughness);
    color = sheen_ibl + color * sheen_scaling;
#endif
#ifdef HAS_CLEARCOAT
    vec3 clearcoat_ibl = scene_reflection(reflect(-v, s.n_coat), s.coat_roughness);
    color = mix(color, clearcoat_ibl, clearcoat * clearcoat_fresnel);
#endif
    color *= s.ao;

    // Punctual lights.
    int count = int(u_lighting.x);
    for (int i = 0; i < MAX_LIGHTS; i++) {
        if (i >= count) break;
        Light light = u_lights[i];
        vec3 to_light = light_vector(light, s.position);
        vec3 l = normalize(to_light);
        vec3 h = normalize(l + v);
        float NdotL = clamp(dot(n, l), 0.0, 1.0);
        float NdotH = clamp(dot(n, h), 0.0, 1.0);
        float VdotH = clamp(dot(v, h), 0.0, 1.0);
        vec3 dielectric_fresnel = F_Schlick(f0_dielectric * specular_weight, f90_dielectric, VdotH);
        vec3 metal_fresnel = F_Schlick(base, vec3(1.0), VdotH);
        vec3 intensity = light.color.rgb * light.color.w * light_attenuation(light, to_light);

        vec3 l_diffuse = intensity * NdotL * base * INV_PI;
#ifdef HAS_DIFFUSE_TRANSMISSION
        l_diffuse *= 1.0 - s.diffuse_transmission;
        if (dot(n, l) < 0.0) {
            vec3 btdf = diffuse_through_volume(
                intensity * clamp(dot(-n, l), 0.0, 1.0) * s.diffuse_transmission_color * INV_PI, s);
            // Its Fresnel as for the light mirrored to the eye's side.
            vec3 l_mirror = reflect(l, n);
            float m_VdotH = clamp(dot(v, normalize(l_mirror + v)), 0.0, 1.0);
            dielectric_fresnel = F_Schlick(f0_dielectric * specular_weight, f90_dielectric, m_VdotH);
            l_diffuse += btdf * s.diffuse_transmission;
        }
#endif
#ifdef HAS_TRANSMISSION
        // Through the volume the light is met where the refracted view ray
        // leaves it.
        vec3 to_exit = to_light;
#ifdef HAS_VOLUME
        vec3 ray = volume_ray(n, v, s.thickness, s.ior);
        if (light.params.w > 0.5) to_exit -= ray; // a directional light is infinitely far
#endif
        vec3 transmitted = intensity * base
            * punctual_transmission(n, v, normalize(to_exit), alpha, s.ior);
#ifdef HAS_VOLUME
        transmitted = volume_attenuation(transmitted, length(ray), s.attenuation_color,
                                         s.attenuation_distance);
#endif
        l_diffuse = mix(l_diffuse, transmitted, s.transmission);
#endif
#ifdef HAS_ANISOTROPY
        // Khronos BRDF_specularGGXAnisotropy: the tangent roughness grows
        // with the strength, the bitangent keeps the material's.
        float at = mix(alpha, 1.0, s.anisotropy * s.anisotropy);
        float ab = clamp(alpha, 0.001, 1.0);
        vec3 t = s.anisotropy_t;
        vec3 b = s.anisotropy_b;
        float spec = V_GGX_aniso(NdotL, NdotV, dot(b, v), dot(t, v), dot(t, l), dot(b, l), at, ab)
            * D_GGX_aniso(clamp(NdotH, 0.001, 1.0), dot(t, h), dot(b, h), at, ab);
        vec3 l_specular = intensity * NdotL * vec3(spec);
#else
        vec3 l_specular = intensity * NdotL * vec3(V_GGX(NdotL, NdotV, alpha) * D_GGX(NdotH, alpha));
#endif
        vec3 l_metal = metal_fresnel * l_specular;
        vec3 l_dielectric = mix(l_diffuse, l_specular, dielectric_fresnel);
#ifdef HAS_IRIDESCENCE
        l_metal = mix(l_metal, l_specular * irid_metal, iridescence);
        l_dielectric = mix(l_dielectric, rgb_mix(l_diffuse, l_specular, irid_dielectric),
                           iridescence);
#endif
        vec3 l_color = mix(l_dielectric, l_metal, s.metallic);
#ifdef HAS_SHEEN
        vec3 l_sheen = intensity * NdotL * s.sheen_color
            * D_Charlie(s.sheen_roughness, NdotH) * V_Sheen(NdotL, NdotV, s.sheen_roughness);
        float l_scaling = min(1.0 - sheen_max * gltf_sheen_albedo(NdotV, s.sheen_roughness),
                              1.0 - sheen_max * gltf_sheen_albedo(NdotL, s.sheen_roughness));
        l_color = l_sheen + l_color * l_scaling;
#endif
#ifdef HAS_CLEARCOAT
        float c_NdotL = clamp(dot(s.n_coat, l), 0.0, 1.0);
        float c_NdotV = clamp(dot(s.n_coat, v), 0.0, 1.0);
        float c_NdotH = clamp(dot(s.n_coat, h), 0.0, 1.0);
        vec3 l_clearcoat = intensity * c_NdotL
            * V_GGX(c_NdotL, c_NdotV, coat_alpha) * D_GGX(c_NdotH, coat_alpha);
        l_color = mix(l_color, l_clearcoat, clearcoat * clearcoat_fresnel);
#endif
        color += l_color;
    }

    return s.emission * (1.0 - clearcoat * clearcoat_fresnel) + color;
}
