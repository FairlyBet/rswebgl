#version 300 es

// ============================================================================
// glTF 2.0 metallic-roughness — FRAGMENT stage (reference shader)
//
// Declares the complete material interface a glTF metallic-roughness material
// can drive (glTF 2.0 §3.9): the PBR factors + textures, normal/occlusion/
// emissive maps, alpha coverage, and the color render targets. Every optional
// input is gated behind a feature `#define` the host injects after `#version`.
//
// IMPORTANT: the actual BRDF is still a PLACEHOLDER. Everything is *declared*
// and wired up to the right sources here (so material data round-trips and the
// model is visible), but `shade()` is a stand-in to be replaced with a real
// Cook-Torrance metallic-roughness evaluation + IBL later. Search "TODO(pbr)".
//
// Recognized defines (host enables the ones that apply):
//   Geometry (must match main.vert):
//     HAS_NORMAL_VEC3, HAS_TANGENT_VEC4,
//     HAS_TEXCOORD_0_VEC2, HAS_TEXCOORD_1_VEC2,
//     HAS_COLOR_0_VEC3 | HAS_COLOR_0_VEC4
//   Material maps (each with an optional `*_UV` override -> v_uv0 | v_uv1):
//     HAS_BASE_COLOR_MAP          BASE_COLOR_UV
//     HAS_METALLIC_ROUGHNESS_MAP  METALLIC_ROUGHNESS_UV
//     HAS_NORMAL_MAP              NORMAL_UV
//     HAS_OCCLUSION_MAP           OCCLUSION_UV
//     HAS_EMISSIVE_MAP            EMISSIVE_UV
//   Alpha coverage (§3.9.4): exactly one of
//     ALPHAMODE_OPAQUE | ALPHAMODE_MASK | ALPHAMODE_BLEND
//   Shading model:
//     MATERIAL_UNLIT              KHR_materials_unlit (skip lighting)
//     USE_PUNCTUAL                evaluate the Lights UBO
//     USE_IBL                     image-based ambient (declares IBL samplers)
//   Extra render targets (MRT; default is a single color output):
//     OUTPUT_EMISSIVE_TARGET      location 1: emissive-only (bloom)
//     OUTPUT_NORMAL_TARGET        location 2: world normal (deferred / SSAO)
// ============================================================================

precision highp float;
precision highp int;

// --- Varyings from the vertex stage -----------------------------------------
in vec3 v_world_position;
#ifdef HAS_NORMAL_VEC3
in vec3 v_normal;
#ifdef HAS_TANGENT_VEC4
in vec3 v_tangent;
in vec3 v_bitangent;
#endif
#endif
in vec2 v_uv0;
in vec2 v_uv1;
#if defined(HAS_COLOR_0_VEC3) || defined(HAS_COLOR_0_VEC4)
in vec4 v_color0;
#endif

// --- Camera (shared with the vertex stage) ----------------------------------
layout(std140) uniform Frame {
    mat4 u_view_proj;
    vec4 u_camera_position; // world-space eye in .xyz
};

// --- Material factors (std140 block) ----------------------------------------
// Mirrors src/material.rs. emissive_factor.w carries the emissive strength
// (KHR_materials_emissive_strength; default 1.0).
layout(std140) uniform Material {
    vec4  u_base_color_factor;
    vec4  u_emissive_factor;     // .rgb factor, .w strength
    float u_metallic_factor;
    float u_roughness_factor;
    float u_normal_scale;        // normalTexture.scale
    float u_occlusion_strength;  // occlusionTexture.strength
    float u_alpha_cutoff;        // used only when ALPHAMODE_MASK
};

// --- Material textures (samplers can't live in a UBO) ------------------------
// Each map samples a configurable TEXCOORD set; default UV0 unless overridden.
#ifdef HAS_BASE_COLOR_MAP
uniform sampler2D u_base_color_texture;
#ifndef BASE_COLOR_UV
#define BASE_COLOR_UV v_uv0
#endif
#endif
#ifdef HAS_METALLIC_ROUGHNESS_MAP
uniform sampler2D u_metallic_roughness_texture; // G = roughness, B = metallic
#ifndef METALLIC_ROUGHNESS_UV
#define METALLIC_ROUGHNESS_UV v_uv0
#endif
#endif
#ifdef HAS_NORMAL_MAP
uniform sampler2D u_normal_texture;
#ifndef NORMAL_UV
#define NORMAL_UV v_uv0
#endif
#endif
#ifdef HAS_OCCLUSION_MAP
uniform sampler2D u_occlusion_texture; // R channel
#ifndef OCCLUSION_UV
#define OCCLUSION_UV v_uv0
#endif
#endif
#ifdef HAS_EMISSIVE_MAP
uniform sampler2D u_emissive_texture;
#ifndef EMISSIVE_UV
#define EMISSIVE_UV v_uv0
#endif
#endif

// --- Punctual lights (KHR_lights_punctual, §3) -------------------------------
#ifdef USE_PUNCTUAL
#ifndef MAX_LIGHTS
#define MAX_LIGHTS 8
#endif
// type: 0 = directional, 1 = point, 2 = spot.
struct Light {
    vec4 position;     // .xyz world position (point/spot)
    vec4 direction;    // .xyz world direction (directional/spot)
    vec4 color;        // .rgb color, .w intensity
    vec4 params;       // x range, y innerConeCos, z outerConeCos, w type
};
layout(std140) uniform Lights {
    Light u_lights[MAX_LIGHTS];
    int   u_light_count;
};
#endif

// --- Image-based lighting ----------------------------------------------------
#ifdef USE_IBL
uniform samplerCube u_ibl_irradiance;  // diffuse irradiance
uniform samplerCube u_ibl_prefiltered; // specular prefiltered env (mip = roughness)
uniform sampler2D   u_ibl_brdf_lut;    // split-sum BRDF integration LUT
uniform float       u_ibl_intensity;
#endif

// --- Color render targets ----------------------------------------------------
layout(location = 0) out vec4 g_final_color;
#ifdef OUTPUT_EMISSIVE_TARGET
layout(location = 1) out vec4 g_emissive_color;
#endif
#ifdef OUTPUT_NORMAL_TARGET
layout(location = 2) out vec4 g_normal_color;
#endif

// ---------------------------------------------------------------------------
// Material sampling helpers
// ---------------------------------------------------------------------------

// Base color: factor * vertex color * sRGB base-color texture (§3.9.2).
// The base-color texture is uploaded as sRGB, so `texture()` already linearizes.
vec4 base_color() {
    vec4 c = u_base_color_factor;
#if defined(HAS_COLOR_0_VEC3) || defined(HAS_COLOR_0_VEC4)
    c *= v_color0;
#endif
#ifdef HAS_BASE_COLOR_MAP
    c *= texture(u_base_color_texture, BASE_COLOR_UV);
#endif
    return c;
}

// Returns (metallic, roughness), texture overriding the factors (§3.9.2).
vec2 metallic_roughness() {
    float metallic = u_metallic_factor;
    float roughness = u_roughness_factor;
#ifdef HAS_METALLIC_ROUGHNESS_MAP
    vec4 mr = texture(u_metallic_roughness_texture, METALLIC_ROUGHNESS_UV);
    roughness *= mr.g;
    metallic *= mr.b;
#endif
    return vec2(clamp(metallic, 0.0, 1.0), clamp(roughness, 0.0, 1.0));
}

// Shading normal in world space, applying the tangent-space normal map (§3.9.3)
// and flipping for back faces of double-sided materials.
vec3 shading_normal() {
#ifdef HAS_NORMAL_VEC3
    vec3 n = normalize(v_normal);
#ifdef HAS_NORMAL_MAP
#ifdef HAS_TANGENT_VEC4
    vec3 t = normalize(v_tangent);
    vec3 b = normalize(v_bitangent);
    vec3 sampled = texture(u_normal_texture, NORMAL_UV).xyz * 2.0 - 1.0;
    sampled.xy *= u_normal_scale;
    n = normalize(mat3(t, b, n) * sampled);
#endif
#endif
    return gl_FrontFacing ? n : -n;
#else
    // No vertex normals: derive a flat normal from screen-space derivatives.
    vec3 n = normalize(cross(dFdx(v_world_position), dFdy(v_world_position)));
    return gl_FrontFacing ? n : -n;
#endif
}

float occlusion() {
#ifdef HAS_OCCLUSION_MAP
    float ao = texture(u_occlusion_texture, OCCLUSION_UV).r;
    return 1.0 + u_occlusion_strength * (ao - 1.0); // §3.9.3
#else
    return 1.0;
#endif
}

vec3 emissive() {
    vec3 e = u_emissive_factor.rgb * u_emissive_factor.w;
#ifdef HAS_EMISSIVE_MAP
    e *= texture(u_emissive_texture, EMISSIVE_UV).rgb; // emissive map is sRGB
#endif
    return e;
}

// ---------------------------------------------------------------------------
// Lighting (PLACEHOLDER)
// ---------------------------------------------------------------------------
// TODO(pbr): replace with a real metallic-roughness BRDF — Cook-Torrance
// specular (GGX distribution, Smith visibility, Fresnel-Schlick) + Lambertian
// diffuse, summed over the Lights UBO and the IBL ambient term. For now this is
// a stand-in so the geometry/material plumbing is visible and correct.
vec3 shade(vec3 albedo, float metallic, float roughness, vec3 n, vec3 v, float ao) {
#ifdef MATERIAL_UNLIT
    return albedo; // KHR_materials_unlit
#else
    // Placeholder: a single hard-coded key light + flat ambient, no specular.
    vec3 l = normalize(vec3(0.4, 0.8, 0.6));
    float ndl = max(dot(n, l), 0.0);
    vec3 diffuse = albedo * (0.25 + 0.75 * ndl);
    return diffuse * ao;
#endif
}

void main() {
    vec4 color = base_color();

    // Alpha coverage, glTF §3.9.4.
#ifdef ALPHAMODE_MASK
    if (color.a < u_alpha_cutoff) {
        discard;
    }
    color.a = 1.0;
#endif
#ifdef ALPHAMODE_OPAQUE
    color.a = 1.0;
#endif

    vec2 mr = metallic_roughness();
    vec3 n = shading_normal();
    vec3 v = normalize(u_camera_position.xyz - v_world_position);

    vec3 lit = shade(color.rgb, mr.x, mr.y, n, v, occlusion());
    vec3 emit = emissive();
    vec3 rgb = lit + emit;

    g_final_color = vec4(rgb, color.a);
#ifdef OUTPUT_EMISSIVE_TARGET
    g_emissive_color = vec4(emit, color.a);
#endif
#ifdef OUTPUT_NORMAL_TARGET
    g_normal_color = vec4(n * 0.5 + 0.5, 1.0);
#endif
}
