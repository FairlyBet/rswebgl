// glTF 2.0 metallic-roughness — fragment stage: the BRDF of the glTF spec
// (appendix B: GGX distribution, height-correlated Smith visibility, Schlick
// Fresnel, Lambert diffuse), punctual lights (KHR_lights_punctual units),
// a uniform ambient light, emissive, alpha modes, then exposure, the Khronos
// PBR Neutral tone mapper and sRGB encoding for the canvas.
//
// Defines (`PbrShaders` adds the ones that apply):
//   geometry   HAS_NORMAL HAS_UV0 HAS_TANGENT HAS_COLOR HAS_UV1
//   maps       HAS_BASE_COLOR_MAP HAS_METALLIC_ROUGHNESS_MAP HAS_NORMAL_MAP
//              HAS_OCCLUSION_MAP HAS_EMISSIVE_MAP, each with <MAP>_UV set to
//              v_uv0 or v_uv1
//   alpha      ALPHAMODE_OPAQUE | ALPHAMODE_MASK | ALPHAMODE_BLEND
//   shading    MATERIAL_UNLIT (KHR_materials_unlit)

precision highp float;

in vec3 v_world;
in vec2 v_uv0;
in vec2 v_uv1;
#ifdef HAS_NORMAL
in vec3 v_normal;
#ifdef HAS_TANGENT
in vec4 v_tangent;
#endif
#endif
#ifdef HAS_COLOR
in vec4 v_color;
#endif

layout(std140) uniform Frame {
    mat4 u_view;
    mat4 u_proj;
    mat4 u_view_proj;
    vec4 u_camera_pos;
};

// Lights: the scene's lighting block (rswebgl-scene `LIGHTING_BLOCK`).
#define MAX_LIGHTS 8
struct Light {
    vec4 position;  // xyz world position (point, spot)
    vec4 direction; // xyz world direction the light points (directional, spot)
    vec4 color;     // rgb color, w intensity (lux for directional, candela else)
    vec4 params;    // x range (0 = infinite), y spot scale, z spot offset, w type
};
layout(std140) uniform Lighting {
    Light u_lights[MAX_LIGHTS];
    vec4 u_ambient;  // rgb ambient radiance
    vec4 u_lighting; // x light count, y exposure
};

// Material: the loader's material group (rswebgl-gltf).
uniform vec4 u_base_color_factor;
uniform float u_metallic_factor;
uniform float u_roughness_factor;
uniform vec3 u_emissive_factor;
uniform float u_emissive_strength;
uniform float u_normal_scale;
uniform float u_occlusion_strength;
uniform float u_alpha_cutoff;
#ifdef HAS_BASE_COLOR_MAP
uniform sampler2D u_base_color_tex;
#endif
#ifdef HAS_METALLIC_ROUGHNESS_MAP
uniform sampler2D u_metallic_roughness_tex; // G roughness, B metallic
#endif
#ifdef HAS_NORMAL_MAP
uniform sampler2D u_normal_tex;
#endif
#ifdef HAS_OCCLUSION_MAP
uniform sampler2D u_occlusion_tex; // R
#endif
#ifdef HAS_EMISSIVE_MAP
uniform sampler2D u_emissive_tex;
#endif

out vec4 frag_color;

const float PI = 3.14159265359;

vec3 to_srgb(vec3 c) {
    c = clamp(c, 0.0, 1.0);
    return mix(c * 12.92, 1.055 * pow(c, vec3(1.0 / 2.4)) - 0.055, step(0.0031308, c));
}

// Khronos PBR Neutral tone mapper.
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

vec4 base_color() {
    vec4 c = u_base_color_factor;
#ifdef HAS_COLOR
    c *= v_color;
#endif
#ifdef HAS_BASE_COLOR_MAP
    c *= texture(u_base_color_tex, BASE_COLOR_UV); // sRGB texture: linear here
#endif
    return c;
}

// World-space shading normal: the vertex normal (or the face normal without
// one), the normal map through the tangent frame (derived from screen-space
// derivatives when the primitive has no tangents), flipped on back faces.
vec3 shading_normal() {
#ifdef HAS_NORMAL
    vec3 n = normalize(v_normal);
#else
    vec3 n = normalize(cross(dFdx(v_world), dFdy(v_world)));
#endif
    if (!gl_FrontFacing) n = -n;
#ifdef HAS_NORMAL_MAP
    vec3 t;
    vec3 b;
#if defined(HAS_NORMAL) && defined(HAS_TANGENT)
    t = normalize(v_tangent.xyz);
    b = cross(normalize(v_normal), t) * v_tangent.w;
    if (!gl_FrontFacing) { t = -t; b = -b; }
#else
    vec3 dp1 = dFdx(v_world);
    vec3 dp2 = dFdy(v_world);
    vec2 duv1 = dFdx(NORMAL_UV);
    vec2 duv2 = dFdy(NORMAL_UV);
    vec3 dp2perp = cross(dp2, n);
    vec3 dp1perp = cross(n, dp1);
    t = dp2perp * duv1.x + dp1perp * duv2.x;
    b = dp2perp * duv1.y + dp1perp * duv2.y;
    float inv = inversesqrt(max(dot(t, t), dot(b, b)) + 1e-12);
    t *= inv;
    b *= inv;
#endif
    vec3 m = texture(u_normal_tex, NORMAL_UV).xyz * 2.0 - 1.0;
    m.xy *= u_normal_scale;
    n = normalize(mat3(t, b, n) * m);
#endif
    return n;
}

float d_ggx(float n_h, float a2) {
    float f = n_h * n_h * (a2 - 1.0) + 1.0;
    return a2 / (PI * f * f);
}

float v_smith(float n_l, float n_v, float a2) {
    float gv = n_l * sqrt(n_v * n_v * (1.0 - a2) + a2);
    float gl = n_v * sqrt(n_l * n_l * (1.0 - a2) + a2);
    float g = gv + gl;
    return g > 0.0 ? 0.5 / g : 0.0;
}

vec3 f_schlick(vec3 f0, float v_h) {
    return f0 + (1.0 - f0) * pow(1.0 - v_h, 5.0);
}

// Distance and cone falloff of a punctual light (KHR_lights_punctual).
float attenuation(Light light, vec3 to_light) {
    float type = light.params.w;
    if (type < 0.5) return 1.0; // directional
    float d2 = max(dot(to_light, to_light), 1e-8);
    float att = 1.0 / d2;
    float range = light.params.x;
    if (range > 0.0) {
        float r = sqrt(d2) / range;
        att *= clamp(1.0 - r * r * r * r, 0.0, 1.0);
    }
    if (type > 1.5) { // spot
        float cd = dot(normalize(light.direction.xyz), -normalize(to_light));
        float s = clamp(cd * light.params.y + light.params.z, 0.0, 1.0);
        att *= s * s;
    }
    return att;
}

// Split-sum environment BRDF, analytic fit (Karis, mobile): stands in for a
// BRDF lookup table until image-based lighting arrives.
vec3 env_brdf(vec3 f0, float roughness, float n_v) {
    const vec4 c0 = vec4(-1.0, -0.0275, -0.572, 0.022);
    const vec4 c1 = vec4(1.0, 0.0425, 1.04, -0.04);
    vec4 r = roughness * c0 + c1;
    float a004 = min(r.x * r.x, exp2(-9.28 * n_v)) * r.x + r.y;
    vec2 ab = vec2(-1.04, 1.04) * a004 + r.zw;
    return f0 * ab.x + ab.y;
}

void main() {
    vec4 base = base_color();
#ifdef ALPHAMODE_MASK
    if (base.a < u_alpha_cutoff) discard;
    base.a = 1.0;
#endif
#ifdef ALPHAMODE_OPAQUE
    base.a = 1.0;
#endif

#ifdef MATERIAL_UNLIT
    frag_color = vec4(to_srgb(base.rgb), base.a);
#else
    float metallic = u_metallic_factor;
    float roughness = u_roughness_factor;
#ifdef HAS_METALLIC_ROUGHNESS_MAP
    vec4 mr = texture(u_metallic_roughness_tex, METALLIC_ROUGHNESS_UV);
    roughness *= mr.g;
    metallic *= mr.b;
#endif
    metallic = clamp(metallic, 0.0, 1.0);
    roughness = clamp(roughness, 0.03, 1.0);
    float a2 = roughness * roughness * roughness * roughness;

    vec3 c_diff = mix(base.rgb, vec3(0.0), metallic);
    vec3 f0 = mix(vec3(0.04), base.rgb, metallic);

    vec3 n = shading_normal();
    vec3 v = normalize(u_camera_pos.xyz - v_world);
    float n_v = clamp(abs(dot(n, v)), 1e-4, 1.0);

    vec3 color = vec3(0.0);
    int count = int(u_lighting.x);
    for (int i = 0; i < MAX_LIGHTS; i++) {
        if (i >= count) break;
        Light light = u_lights[i];
        vec3 to_light = light.params.w < 0.5
            ? -light.direction.xyz
            : light.position.xyz - v_world;
        vec3 l = normalize(to_light);
        float n_l = clamp(dot(n, l), 0.0, 1.0);
        if (n_l <= 0.0) continue;
        vec3 h = normalize(l + v);
        float n_h = clamp(dot(n, h), 0.0, 1.0);
        float v_h = clamp(dot(v, h), 0.0, 1.0);
        vec3 f = f_schlick(f0, v_h);
        vec3 diffuse = (1.0 - f) * c_diff / PI;
        vec3 specular = f * d_ggx(n_h, a2) * v_smith(n_l, n_v, a2);
        vec3 radiance = light.color.rgb * light.color.w * attenuation(light, to_light);
        color += (diffuse + specular) * radiance * n_l;
    }

    float ao = 1.0;
#ifdef HAS_OCCLUSION_MAP
    ao = 1.0 + u_occlusion_strength * (texture(u_occlusion_tex, OCCLUSION_UV).r - 1.0);
#endif
    color += u_ambient.rgb * (c_diff + env_brdf(f0, roughness, n_v)) * ao;

    vec3 emissive = u_emissive_factor * u_emissive_strength;
#ifdef HAS_EMISSIVE_MAP
    emissive *= texture(u_emissive_tex, EMISSIVE_UV).rgb;
#endif
    color += emissive;

    frag_color = vec4(to_srgb(tone_map(color * u_lighting.y)), base.a);
#endif
}
