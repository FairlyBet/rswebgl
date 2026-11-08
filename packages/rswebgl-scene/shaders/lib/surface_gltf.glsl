// The glTF 2.0 reference shading model: what the Khronos sample renderer
// computes, for comparing against OpenPBR on the same material. Base
// metallic-roughness (glTF spec appendix B), KHR_materials_ior, _specular,
// _clearcoat, _transmission, _volume, _dispersion, _emissive_strength.
// Needs brdf.glsl, surface.glsl, lights.glsl, environment.glsl.
//
// Ported from the Khronos glTF-Sample-Renderer (Apache-2.0),
// source/Renderer/shaders/pbr.frag, ibl.glsl, punctual.glsl,
// material_info.glsl. Differences: the environment's transmission stands in
// for the transmission framebuffer (the scene behind isn't seen yet).

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

    // Image-based light.
    vec3 f_diffuse = scene_irradiance(n) * base;
#ifdef HAS_TRANSMISSION
    vec3 f_transmission = env_transmission(s, s.ior, roughness) * base;
    f_diffuse = mix(f_diffuse, f_transmission, s.transmission);
#endif
    vec3 f_specular = scene_reflection(reflect(-v, n), roughness);
    vec3 metal_ibl = gltf_ibl_fresnel(NdotV, roughness, base, 1.0) * f_specular;
    vec3 dielectric_fresnel_ibl = gltf_ibl_fresnel(NdotV, roughness, f0_dielectric, specular_weight);
    vec3 dielectric_ibl = mix(f_diffuse, f_specular, dielectric_fresnel_ibl);
    vec3 color = mix(dielectric_ibl, metal_ibl, s.metallic);
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
#ifdef HAS_TRANSMISSION
        // Through the volume the light is met where the refracted view ray
        // leaves it.
        vec3 to_exit = to_light;
#ifdef HAS_VOLUME
        vec3 ray = volume_ray(n, v, s.thickness, s.ior);
        to_exit -= ray;
#endif
        vec3 transmitted = intensity * base
            * punctual_transmission(n, v, normalize(to_exit), alpha, s.ior);
#ifdef HAS_VOLUME
        transmitted = volume_attenuation(transmitted, length(ray), s.attenuation_color,
                                         s.attenuation_distance);
#endif
        l_diffuse = mix(l_diffuse, transmitted, s.transmission);
#endif
        vec3 l_specular = intensity * NdotL * vec3(V_GGX(NdotL, NdotV, alpha) * D_GGX(NdotH, alpha));
        vec3 l_metal = metal_fresnel * l_specular;
        vec3 l_dielectric = mix(l_diffuse, l_specular, dielectric_fresnel);
        vec3 l_color = mix(l_dielectric, l_metal, s.metallic);
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
