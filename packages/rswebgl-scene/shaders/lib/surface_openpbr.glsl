// The OpenPBR Surface shading model (OpenPBR spec v1.1, Academy Software
// Foundation): fuzz over a coat over a base substrate that mixes a
// dielectric (specular over diffuse or transmission) with an F82-tint metal,
// thin film on both, layered by albedo scaling. Needs brdf.glsl, eon.glsl,
// sheen.glsl, iridescence.glsl, surface.glsl, lights.glsl, environment.glsl.
//
// Follows the spec where it is explicit and MaterialX's reference graph
// (libraries/bxdf/open_pbr_surface.mtlx, Apache-2.0) where it leaves room:
// - the dielectric's specular_weight modulates its IOR, and the exact
//   Fresnel of that IOR shades it; specular_color tints reflection only;
// - every layer passes on (1 − its directional albedo), from MaterialX's
//   analytic GGX fit with Turquin's multiple-scattering compensation;
// - diffuse is EON (energy-preserving Oren-Nayar), Lambert at roughness 0;
// - anisotropy: α_t = α √(2 / (1 + (1 − a)²)), α_b = (1 − a) α_t, the
//   albedo at their geometric mean; image-based light along a bent normal;
// - thin film (Belcour-Barla, MaterialX's Airy form) on the dielectric's
//   and the metal's reflection, its albedo blended from the mirror value
//   to the rough fit by √α (MaterialX), not on transmission;
// - the coat roughens the base (min(1, r⁴ + 2r_c⁴)^¼), absorbs with a
//   view-dependent path (spec "view-dependent absorption", unlike
//   MaterialX's constant), and darkens it (spec Δ with the E_F fit);
// - fuzz: Zeltner's LTC sheen on top of everything, gray albedo scaling,
//   emission not attenuated (as MaterialX);
// - image-based light: split-sum prefiltered specular × directional
//   albedo, irradiance × EON / fuzz albedo; ambient occlusion also occludes
//   specular (Lagarde).
// glTF inputs map onto it: sheen → fuzz (weight = max of the color),
// iridescence → thin film, anisotropy → specular anisotropy.
// - the coat's anisotropy as the specular's (MaterialX open_pbr_anisotropy),
//   along the surface tangent;
// - subsurface: MaterialX's real-time approximation (subsurface.glsl) —
//   direct light through a curvature-sized Burley profile, indirect as
//   irradiance × subsurface_color — mixed into the diffuse by its weight;
//   no scatter anisotropy, not thin-walled.

struct OpenPbr {
    float alpha;          // base specular α (roughened by the coat; mean of the two below)
    float alpha_t;        // along the anisotropy direction
    float alpha_b;
    float roughness;      // its perceptual roughness
    float eta;            // the dielectric's IOR after specular_weight (relative to the coat)
    float eta_refract;    // what refraction bends by: the base's own (after specular_weight)
    float f0_dielectric;
    vec3 comp_dielectric; // multiple-scattering compensation
    vec3 fg_dielectric;   // directional albedo (compensated, untinted, film blended in)
    vec3 f0_metal;
    vec3 comp_metal;
    vec3 fg_metal;        // the metal's directional albedo (× specular_weight)
    float film;           // thin-film weight (0 without a film)
    vec3 tint;            // translucent base: thin-walled tint
    float coat_alpha;     // the coat's α (mean of the two below)
    float coat_alpha_t;
    float coat_alpha_b;
    float coat_eta;
    float coat_f0;
    float coat_comp;
    float coat_e;         // the coat's directional albedo (view)
    vec3 coat_t;          // single-pass coat transmittance at normal incidence
    vec3 darkening;       // the coat's darkening of the base, lerped in
    float fuzz;           // fuzz weight
    vec3 fuzz_color;
    float fuzz_e;         // the fuzz's directional albedo (view)
};

// Hemispherical average of dielectric Fresnel (spec [^avg_fresnel] fit,
// η ∈ [1, 3]; reciprocal relation below 1).
float avg_fresnel(float eta) {
    float e = clamp(max(eta, 1.0 / eta), 1.0, 3.0); // the fit's range
    float ef = log((10893.0 * e - 1438.2) / (-774.4 * e * e + 10212.0 * e + 1.0));
    return eta >= 1.0 ? ef : 1.0 - eta * eta * (1.0 - ef);
}

// Cosine of the refracted angle inside a layer of relative IOR `eta`.
float refracted_cos(float mu, float eta) {
    return sqrt(max(1e-4, 1.0 - (1.0 - mu * mu) / (eta * eta)));
}

// The film's Fresnel over the dielectric or the metal at cosine `mu`.
vec3 film_dielectric(Surface s, OpenPbr o, float mu) {
    return openpbr_thin_film(mu, s.iridescence_ior, s.iridescence_thickness,
                             vec3(o.f0_dielectric), vec3(1.0), true, o.eta);
}

vec3 film_metal(Surface s, OpenPbr o, float mu) {
    return openpbr_thin_film(mu, s.iridescence_ior, s.iridescence_thickness,
                             o.f0_metal, s.specular_color, false, 1.0);
}

OpenPbr openpbr_setup(Surface s, float NdotV, float coat_NdotV) {
    OpenPbr o;
    float r = s.roughness;
#ifdef HAS_CLEARCOAT
    // The coat roughens what is under it.
    float r4 = r * r * r * r;
    float rc = s.coat_roughness;
    r = mix(r, sqrt(sqrt(min(1.0, r4 + 2.0 * rc * rc * rc * rc))), s.coat);
#endif
    o.roughness = r;
    float alpha = max(r * r, 1e-4);
    o.alpha_t = alpha;
    o.alpha_b = alpha;
#ifdef HAS_ANISOTROPY
    float a1 = 1.0 - s.anisotropy;
    o.alpha_t = alpha * sqrt(2.0 / (1.0 + a1 * a1));
    o.alpha_b = max(a1 * o.alpha_t, 1e-4);
    alpha = sqrt(o.alpha_t * o.alpha_b);
#endif
    o.alpha = alpha;

    // The dielectric's IOR relative to what is above it (coat or air), with
    // the spec's TIR fix, then modulated by specular_weight.
    float eta_s = s.ior;
#ifdef HAS_CLEARCOAT
    float ratio = s.ior / s.coat_ior;
    eta_s = mix(s.ior, max(ratio, 1.0 / ratio), s.coat);
#endif
    float f_s = ior_to_f0(eta_s);
    float xi_f = clamp(s.specular_weight * f_s, 0.0, 0.99999);
    float eps = sign(eta_s - 1.0) * sqrt(xi_f);
    o.eta = (1.0 + eps) / (1.0 - eps);
    // Light crossing a parallel coat into the base bends as if from air
    // (Snell's law chains): the base's IOR alone, specular_weight applied.
    float xi_r = clamp(s.specular_weight * ior_to_f0(s.ior), 0.0, 0.99999);
    float eps_r = sign(s.ior - 1.0) * sqrt(xi_r);
    o.eta_refract = (1.0 + eps_r) / (1.0 - eps_r);
    o.f0_dielectric = xi_f;
    o.comp_dielectric = ggx_energy_compensation(NdotV, alpha, vec3(xi_f), vec3(1.0));
    o.fg_dielectric = ggx_dir_albedo(NdotV, alpha, vec3(xi_f), vec3(1.0)) * o.comp_dielectric;

    o.f0_metal = s.base_weight * s.base_color;
    o.comp_metal = ggx_energy_compensation(NdotV, alpha, o.f0_metal, vec3(1.0));
    o.fg_metal = s.specular_weight * ggx_dir_albedo(NdotV, alpha, o.f0_metal, vec3(1.0))
        * o.comp_metal;

    o.film = 0.0;
#ifdef HAS_IRIDESCENCE
    o.film = s.iridescence_thickness > 0.0 ? s.iridescence : 0.0;
    // Mirror albedo = the film's Fresnel at the view; rough = the fit with
    // its normal-incidence value; blended by √α (MaterialX).
    float blend = sqrt(alpha);
    vec3 fg_film_d = mix(film_dielectric(s, o, NdotV),
                         ggx_dir_albedo(NdotV, alpha, film_dielectric(s, o, 1.0), vec3(1.0)), blend);
    vec3 fg_film_m = mix(film_metal(s, o, NdotV),
                         ggx_dir_albedo(NdotV, alpha, film_metal(s, o, 1.0), vec3(1.0)), blend);
    o.fg_dielectric = mix(o.fg_dielectric, fg_film_d * o.comp_dielectric, o.film);
    o.fg_metal = mix(o.fg_metal, s.specular_weight * fg_film_m * o.comp_metal, o.film);
#endif

    // Transmission is tinted by the base color, with or without a volume
    // (glTF's KHR_materials_transmission; a volume absorbs on top). OpenPBR
    // proper tints by transmission_color alone; glTF-made materials would
    // lose their color under it.
    o.tint = s.base_color;

    o.coat_alpha = 1.0;
    o.coat_alpha_t = 1.0;
    o.coat_alpha_b = 1.0;
    o.coat_eta = 1.0;
    o.coat_f0 = 0.0;
    o.coat_comp = 1.0;
    o.coat_e = 0.0;
    o.coat_t = vec3(1.0);
    o.darkening = vec3(1.0);
#ifdef HAS_CLEARCOAT
    o.coat_alpha = max(s.coat_roughness * s.coat_roughness, 1e-4);
    o.coat_alpha_t = o.coat_alpha;
    o.coat_alpha_b = o.coat_alpha;
    if (s.coat_anisotropy > 0.0) {
        float ca1 = 1.0 - s.coat_anisotropy;
        o.coat_alpha_t = o.coat_alpha * sqrt(2.0 / (1.0 + ca1 * ca1));
        o.coat_alpha_b = max(ca1 * o.coat_alpha_t, 1e-4);
        o.coat_alpha = sqrt(o.coat_alpha_t * o.coat_alpha_b);
    }
    o.coat_eta = s.coat_ior;
    o.coat_f0 = ior_to_f0(s.coat_ior);
    o.coat_comp = ggx_energy_compensation(coat_NdotV, o.coat_alpha, vec3(o.coat_f0), vec3(1.0)).x;
    o.coat_e = ggx_dir_albedo(coat_NdotV, o.coat_alpha, vec3(o.coat_f0), vec3(1.0)).x * o.coat_comp;
    o.coat_t = sqrt(max(s.coat_color, 0.0));
    // Darkening (spec Δ): light the coat reflects back into the base.
    float k_rough = 1.0 - (1.0 - avg_fresnel(s.coat_ior)) / (s.coat_ior * s.coat_ior);
    float k_smooth = fresnel_dielectric(coat_NdotV, s.coat_ior);
    float r_d = mix(1.0, s.roughness, clamp(xi_f, 0.0, 1.0));
    float r_b = mix(r_d, s.roughness, s.metallic);
    float k = mix(k_smooth, k_rough, r_b);
    vec3 e_base = mix(s.base_weight * s.base_color * (1.0 - s.transmission),
                      s.specular_weight * o.f0_metal, s.metallic);
    vec3 delta = (1.0 - k) / (1.0 - e_base * k);
    o.darkening = mix(vec3(1.0), delta, s.coat * s.coat_darkening);
#endif

    o.fuzz = 0.0;
    o.fuzz_color = vec3(1.0);
    o.fuzz_e = 0.0;
#ifdef HAS_SHEEN
    // glTF's sheen color as OpenPBR fuzz: its strongest channel the weight,
    // the color normalized.
    o.fuzz = max3(s.sheen_color);
    o.fuzz_color = o.fuzz > 0.0 ? s.sheen_color / o.fuzz : vec3(1.0);
    o.fuzz_e = fuzz_albedo(NdotV, s.sheen_roughness);
#endif
    return o;
}

// What the coat lets through to the base and back, for light in and out at
// cosines mu_i, mu_o to the coat normal: absorption along both refracted
// paths, the coat's reflection, darkening.
vec3 openpbr_coat_filter(Surface s, OpenPbr o, float mu_i, float mu_o) {
#ifdef HAS_CLEARCOAT
    vec3 absorbed = pow(o.coat_t, vec3(1.0 / refracted_cos(mu_i, o.coat_eta)
                                       + 1.0 / refracted_cos(mu_o, o.coat_eta)));
    return mix(vec3(1.0), absorbed * (1.0 - o.coat_e), s.coat) * o.darkening;
#else
    return vec3(1.0);
#endif
}

vec3 shade_openpbr(Surface s) {
    vec3 n = s.n;
    vec3 v = s.v;
    float NdotV = clamp(dot(n, v), 1e-4, 1.0);
    float c_NdotV = clamp(dot(s.n_coat, v), 1e-4, 1.0);
    OpenPbr o = openpbr_setup(s, NdotV, c_NdotV);
    float diffuse_weight = s.base_weight * (1.0 - s.transmission);

    // Image-based light.
    float spec_ao = specular_occlusion(NdotV, s.ao, o.alpha);
#ifdef HAS_ANISOTROPY
    vec3 r_dir = aniso_reflection(n, v, o.roughness, s.anisotropy, s.anisotropy_b);
#else
    vec3 r_dir = reflect(-v, n);
#endif
    vec3 prefiltered = scene_reflection(r_dir, o.roughness) * spec_ao;
    vec3 dielectric = prefiltered * s.specular_color * o.fg_dielectric;
    vec3 diffuse_ibl = diffuse_albedo(s.base_color, s.diffuse_roughness, NdotV);
#ifdef HAS_SUBSURFACE
    // MaterialX: indirect subsurface as plain irradiance × its color.
    // base_weight scales the diffuse only (MaterialX: the subsurface BSDF
    // has weight 1).
    diffuse_ibl = mix(diffuse_weight * diffuse_ibl,
                      (1.0 - s.transmission) * s.subsurface_color, s.subsurface);
#else
    diffuse_ibl *= diffuse_weight;
#endif
    vec3 under = scene_irradiance(n) * diffuse_ibl * s.ao;
#ifdef HAS_DIFFUSE_TRANSMISSION
    // KHR_materials_diffuse_transmission, as the glTF model: a share of
    // the diffuse is the light from the other side, by its own color.
    vec3 dt_under = diffuse_through_volume(
        scene_irradiance(-n) * s.diffuse_transmission_color, s) * diffuse_weight * s.ao;
    under = mix(under, dt_under, s.diffuse_transmission);
#endif
#ifdef HAS_TRANSMISSION
    under += s.transmission * o.tint * env_transmission(s, o.eta_refract, o.roughness);
#endif
    dielectric += (1.0 - o.fg_dielectric) * under;
    vec3 metal = prefiltered * o.fg_metal;
    vec3 color = mix(dielectric, metal, s.metallic) * openpbr_coat_filter(s, o, c_NdotV, c_NdotV);
#ifdef HAS_CLEARCOAT
    vec3 coat_dir = s.coat_anisotropy > 0.0
        ? aniso_reflection(s.n_coat, v, s.coat_roughness, s.coat_anisotropy, s.coat_b)
        : reflect(-v, s.n_coat);
    color += s.coat * o.coat_e * specular_occlusion(c_NdotV, s.ao, o.coat_alpha)
        * scene_reflection(coat_dir, s.coat_roughness);
#endif
#ifdef HAS_SHEEN
    color = o.fuzz * o.fuzz_color * o.fuzz_e * scene_irradiance(n) * s.ao
        + (1.0 - o.fuzz * o.fuzz_e) * color;
#endif

    // Punctual lights.
    int count = int(u_lighting.x);
    for (int i = 0; i < MAX_LIGHTS; i++) {
        if (i >= count) break;
        Light light = u_lights[i];
        vec3 l;
        vec3 radiance = light_radiance(light, s.position, l);
        vec3 h = normalize(l + v);
        float NdotL = dot(n, l);
        float VdotH = clamp(dot(v, h), 0.0, 1.0);
        vec3 lit = vec3(0.0);
        if (NdotL > 0.0) {
            float NdotH = clamp(dot(n, h), 0.0, 1.0);
#ifdef HAS_ANISOTROPY
            vec3 t = s.anisotropy_t;
            vec3 b = s.anisotropy_b;
            float dv = D_GGX_aniso(NdotH, dot(t, h), dot(b, h), o.alpha_t, o.alpha_b)
                * V_GGX_aniso(NdotL, NdotV, dot(b, v), dot(t, v), dot(t, l), dot(b, l),
                              o.alpha_t, o.alpha_b);
#else
            float dv = D_GGX(NdotH, o.alpha) * V_GGX(NdotL, NdotV, o.alpha);
#endif
            vec3 f_d = vec3(fresnel_dielectric(VdotH, o.eta));
            vec3 f_m = fresnel_f82(VdotH, o.f0_metal, s.specular_color);
#ifdef HAS_IRIDESCENCE
            if (o.film > 0.0) {
                f_d = mix(f_d, film_dielectric(s, o, VdotH), o.film);
                f_m = mix(f_m, film_metal(s, o, VdotH), o.film);
            }
#endif
            vec3 spec_d = dv * f_d * s.specular_color * o.comp_dielectric;
            vec3 diff = diffuse_weight
                * diffuse_brdf(s.base_color, s.diffuse_roughness, NdotL, NdotV, dot(l, v));
#ifdef HAS_DIFFUSE_TRANSMISSION
            diff *= 1.0 - s.diffuse_transmission;
#endif
#ifdef HAS_SUBSURFACE
            diff *= 1.0 - s.subsurface;
#endif
            vec3 spec_m = s.specular_weight * dv * f_m * o.comp_metal;
            lit = mix(spec_d + (1.0 - o.fg_dielectric) * diff, spec_m, s.metallic) * NdotL;
        }
#ifdef HAS_SUBSURFACE
        // Wraps past the terminator: its own cosine, not NdotL's cut.
        vec3 sss = subsurface_scattering(n, l, s.subsurface_color, s.subsurface_mfp,
                                         s.subsurface_radius);
        lit += (1.0 - s.metallic) * (1.0 - o.fg_dielectric) * (1.0 - s.transmission)
            * s.subsurface * sss;
#endif
#ifdef HAS_DIFFUSE_TRANSMISSION
        if (NdotL < 0.0) {
            // Diffusely through, with the Fresnel of the light mirrored to
            // the eye's side (the Khronos sample renderer).
            float m_VdotH = clamp(dot(v, normalize(reflect(l, n) + v)), 0.0, 1.0);
            lit += (1.0 - s.metallic) * (1.0 - fresnel_dielectric(m_VdotH, o.eta))
                * diffuse_weight * s.diffuse_transmission
                * diffuse_through_volume(s.diffuse_transmission_color * INV_PI * -NdotL, s);
        }
#endif
#ifdef HAS_TRANSMISSION
        // Light from behind, through the surface (as glTF's reference does).
        vec3 to_exit = light_vector(light, s.position);
#ifdef HAS_VOLUME
        vec3 ray = volume_ray(n, v, s.thickness, o.eta_refract);
        if (light.params.w > 0.5) to_exit -= ray; // a directional light is infinitely far
#endif
        vec3 t_light = o.tint * punctual_transmission(n, v, normalize(to_exit), o.alpha, o.eta_refract);
#ifdef HAS_VOLUME
        t_light = volume_attenuation(t_light, length(ray), s.attenuation_color,
                                     s.attenuation_distance);
#endif
        lit += (1.0 - s.metallic) * (1.0 - o.fg_dielectric) * s.transmission * t_light;
#endif
        float c_NdotL = clamp(abs(dot(s.n_coat, l)), 1e-4, 1.0);
        lit *= openpbr_coat_filter(s, o, c_NdotL, c_NdotV);
#ifdef HAS_CLEARCOAT
        if (dot(s.n_coat, l) > 0.0) {
            float c_NdotH = clamp(dot(s.n_coat, h), 0.0, 1.0);
            float c_dv = D_GGX(c_NdotH, o.coat_alpha) * V_GGX(c_NdotL, c_NdotV, o.coat_alpha);
            if (s.coat_anisotropy > 0.0) {
                vec3 ct = s.coat_t;
                vec3 cb = s.coat_b;
                c_dv = D_GGX_aniso(c_NdotH, dot(ct, h), dot(cb, h), o.coat_alpha_t, o.coat_alpha_b)
                    * V_GGX_aniso(c_NdotL, c_NdotV, dot(cb, v), dot(ct, v), dot(ct, l), dot(cb, l),
                                  o.coat_alpha_t, o.coat_alpha_b);
            }
            lit += s.coat * c_dv * fresnel_dielectric(VdotH, o.coat_eta) * o.coat_comp * c_NdotL;
        }
#endif
#ifdef HAS_SHEEN
        // The fuzz lobe already includes the cosine.
        lit = o.fuzz * o.fuzz_color * o.fuzz_e * fuzz_lobe(l, v, n, NdotV, s.sheen_roughness)
            + (1.0 - o.fuzz * o.fuzz_e) * lit;
#endif
        color += lit * radiance;
    }

    // Emission from the base, through the coat on the way out (the fuzz
    // doesn't dim it, as in MaterialX).
    vec3 emission = s.emission;
#ifdef HAS_CLEARCOAT
    float coat_out = (1.0 - o.coat_f0) * (1.0 - pow5(1.0 - c_NdotV));
    emission *= mix(vec3(1.0),
                    pow(o.coat_t, vec3(1.0 / refracted_cos(c_NdotV, o.coat_eta))) * coat_out,
                    s.coat);
#endif
    return color + emission;
}
