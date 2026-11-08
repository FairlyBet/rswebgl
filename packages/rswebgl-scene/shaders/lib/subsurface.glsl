// Subsurface scattering, real-time approximation — ported from MaterialX
// (libraries/pbrlib/genglsl/lib/mx_microfacet_diffuse.glsl and
// mx_subsurface_bsdf.glsl, Apache-2.0): the Burley diffusion profile
// (Christensen & Burley 2015, "Approximate Reflectance Profiles for
// Efficient Subsurface Scattering") integrated over a sphere whose radius
// follows the surface's curvature (after Penner 2011, "Pre-Integrated Skin
// Shading"). Light wraps past the terminator where the surface is curved
// relative to the mean free path. Needs common.glsl.

// The profile at distance `dist` for a diffusion shape (1 / mean free path).
vec3 burley_diffusion_profile(float dist, vec3 shape) {
    vec3 num1 = exp(-shape * dist);
    vec3 num2 = exp(-shape * dist / 3.0);
    float denom = max(dist, 1e-6);
    return (num1 + num2) / denom;
}

// The profile integrated around a sphere of `radius` lit from `l`, against
// the plain cosine: what of the light reaches the point at n.
vec3 integrate_burley_diffusion(vec3 n, vec3 l, float radius, vec3 mfp) {
    float theta = acos(clamp(dot(n, l), -1.0, 1.0));
    vec3 shape = vec3(1.0) / max(mfp, vec3(0.1));
    vec3 sum_d = vec3(0.0);
    vec3 sum_r = vec3(0.0);
    const int SAMPLES = 32;
    const float WIDTH = 2.0 * PI / float(SAMPLES);
    for (int i = 0; i < SAMPLES; i++) {
        float x = -PI + (float(i) + 0.5) * WIDTH;
        float dist = radius * abs(2.0 * sin(x * 0.5));
        vec3 r = burley_diffusion_profile(dist, shape);
        sum_d += r * max(cos(theta + x), 0.0);
        sum_r += r;
    }
    return sum_d / sum_r;
}

// The subsurface lobe for light from `l`, cosine included: albedo / π times
// the diffused cosine. `radius`: the curvature radius at the point.
vec3 subsurface_scattering(vec3 n, vec3 l, vec3 albedo, vec3 mfp, float radius) {
    return albedo * integrate_burley_diffusion(n, l, radius, mfp) * INV_PI;
}
