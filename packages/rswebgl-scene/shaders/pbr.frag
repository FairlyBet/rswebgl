// The built-in material shader: glTF metallic-roughness inputs (factors,
// maps, extensions) and OpenPBR's own parameters, shaded by one of two
// models — OpenPBR Surface (`surface_openpbr.glsl`, the default) or the glTF
// reference (`surface_gltf.glsl`, MODEL_GLTF) — then alpha modes and
// `scene_output`. The scene prelude (camera, lights, environment, object)
// comes first.
//
// Defines (`PbrShaders` adds the ones that apply):
//   model      MODEL_GLTF (else OpenPBR)
//   geometry   HAS_NORMAL HAS_UV0 HAS_TANGENT HAS_COLOR HAS_UV1
//   maps       HAS_BASE_COLOR_MAP HAS_METALLIC_ROUGHNESS_MAP HAS_NORMAL_MAP
//              HAS_OCCLUSION_MAP HAS_EMISSIVE_MAP, each with <MAP>_UV set to
//              v_uv0 or v_uv1
//   alpha      ALPHAMODE_OPAQUE | ALPHAMODE_MASK | ALPHAMODE_BLEND
//   shading    MATERIAL_UNLIT (KHR_materials_unlit)
//   layers     HAS_TRANSMISSION HAS_VOLUME HAS_CLEARCOAT HAS_DISPERSION
//              HAS_DIFFUSE_ROUGHNESS HAS_SHEEN HAS_IRIDESCENCE HAS_ANISOTROPY — a layer the material doesn't use isn't
//              compiled at all

// The model's scale per axis, from the vertex stage (volume thickness is in
// mesh units: surface.glsl).
flat in vec3 v_model_scale;

#include "brdf.glsl"
#include "eon.glsl"
#include "subsurface.glsl"
#include "sheen.glsl"
#include "iridescence.glsl"
#include "surface.glsl"
#include "surface_gltf.glsl"
#include "surface_openpbr.glsl"

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
// KHR_materials_ior, _specular; OpenPBR base_weight.
uniform float u_ior;
uniform float u_specular_factor;
uniform vec3 u_specular_color;
uniform float u_base_weight;
#ifdef HAS_DIFFUSE_ROUGHNESS
uniform float u_diffuse_roughness;
#endif
#ifdef HAS_TRANSMISSION
uniform float u_transmission;
#endif
#ifdef HAS_VOLUME
uniform float u_thickness;            // mesh units
uniform vec3 u_attenuation_color;
uniform float u_attenuation_distance; // 0 = no absorption
#endif
#ifdef HAS_CLEARCOAT
uniform float u_clearcoat;
uniform float u_clearcoat_roughness;
uniform float u_coat_ior;
uniform vec3 u_coat_color;
uniform float u_coat_darkening;
uniform float u_coat_anisotropy;
#endif
#ifdef HAS_SUBSURFACE
uniform float u_subsurface_weight;
uniform vec3 u_subsurface_color;
uniform float u_subsurface_radius;
uniform vec3 u_subsurface_radius_scale;
#endif
#ifdef HAS_DISPERSION
uniform float u_dispersion;
#endif
#ifdef HAS_SHEEN
uniform vec3 u_sheen_color;
uniform float u_sheen_roughness;
#endif
#ifdef HAS_IRIDESCENCE
uniform float u_iridescence;
uniform float u_iridescence_ior;
uniform float u_iridescence_thickness; // nm
#endif
#ifdef HAS_ANISOTROPY
uniform float u_anisotropy_strength;
uniform float u_anisotropy_rotation;  // radians, from the tangent
#endif
// KHR_texture_transform: each map's UV matrix (its <MAP>_UV applies it).
#if defined(HAS_TEXTURE_TRANSFORM) && defined(HAS_BASE_COLOR_MAP)
uniform mat3 u_base_color_uv;
#endif
#if defined(HAS_TEXTURE_TRANSFORM) && defined(HAS_METALLIC_ROUGHNESS_MAP)
uniform mat3 u_metallic_roughness_uv;
#endif
#if defined(HAS_TEXTURE_TRANSFORM) && defined(HAS_NORMAL_MAP)
uniform mat3 u_normal_uv;
#endif
#if defined(HAS_TEXTURE_TRANSFORM) && defined(HAS_OCCLUSION_MAP)
uniform mat3 u_occlusion_uv;
#endif
#if defined(HAS_TEXTURE_TRANSFORM) && defined(HAS_EMISSIVE_MAP)
uniform mat3 u_emissive_uv;
#endif
#if defined(HAS_TEXTURE_TRANSFORM) && defined(HAS_CLEARCOAT_MAP)
uniform mat3 u_clearcoat_uv;
#endif
#if defined(HAS_TEXTURE_TRANSFORM) && defined(HAS_CLEARCOAT_ROUGHNESS_MAP)
uniform mat3 u_clearcoat_roughness_uv;
#endif
#if defined(HAS_TEXTURE_TRANSFORM) && defined(HAS_CLEARCOAT_NORMAL_MAP)
uniform mat3 u_clearcoat_normal_uv;
#endif
#if defined(HAS_TEXTURE_TRANSFORM) && defined(HAS_TRANSMISSION_MAP)
uniform mat3 u_transmission_uv;
#endif
#if defined(HAS_TEXTURE_TRANSFORM) && defined(HAS_THICKNESS_MAP)
uniform mat3 u_thickness_uv;
#endif
#if defined(HAS_TEXTURE_TRANSFORM) && defined(HAS_SPECULAR_MAP)
uniform mat3 u_specular_uv;
#endif
#if defined(HAS_TEXTURE_TRANSFORM) && defined(HAS_SPECULAR_COLOR_MAP)
uniform mat3 u_specular_color_uv;
#endif
#if defined(HAS_TEXTURE_TRANSFORM) && defined(HAS_SHEEN_COLOR_MAP)
uniform mat3 u_sheen_color_uv;
#endif
#if defined(HAS_TEXTURE_TRANSFORM) && defined(HAS_SHEEN_ROUGHNESS_MAP)
uniform mat3 u_sheen_roughness_uv;
#endif
#if defined(HAS_TEXTURE_TRANSFORM) && defined(HAS_IRIDESCENCE_MAP)
uniform mat3 u_iridescence_uv;
#endif
#if defined(HAS_TEXTURE_TRANSFORM) && defined(HAS_IRIDESCENCE_THICKNESS_MAP)
uniform mat3 u_iridescence_thickness_uv;
#endif
#if defined(HAS_TEXTURE_TRANSFORM) && defined(HAS_ANISOTROPY_MAP)
uniform mat3 u_anisotropy_uv;
#endif

// Extension maps (channels as their extensions define them).
#ifdef HAS_CLEARCOAT_MAP
uniform sampler2D u_clearcoat_tex;              // R
#endif
#ifdef HAS_CLEARCOAT_ROUGHNESS_MAP
uniform sampler2D u_clearcoat_roughness_tex;    // G
#endif
#ifdef HAS_CLEARCOAT_NORMAL_MAP
uniform sampler2D u_clearcoat_normal_tex;
uniform float u_clearcoat_normal_scale;
#endif
#ifdef HAS_TRANSMISSION_MAP
uniform sampler2D u_transmission_tex;           // R
#endif
#ifdef HAS_THICKNESS_MAP
uniform sampler2D u_thickness_tex;              // G
#endif
#ifdef HAS_SPECULAR_MAP
uniform sampler2D u_specular_tex;               // A
#endif
#ifdef HAS_SPECULAR_COLOR_MAP
uniform sampler2D u_specular_color_tex;         // RGB, sRGB
#endif
#ifdef HAS_SHEEN_COLOR_MAP
uniform sampler2D u_sheen_color_tex;            // RGB, sRGB
#endif
#ifdef HAS_SHEEN_ROUGHNESS_MAP
uniform sampler2D u_sheen_roughness_tex;        // A
#endif
#ifdef HAS_IRIDESCENCE_MAP
uniform sampler2D u_iridescence_tex;            // R
#endif
#ifdef HAS_IRIDESCENCE_THICKNESS_MAP
uniform sampler2D u_iridescence_thickness_tex;  // G: minimum → maximum
uniform float u_iridescence_thickness_min;
#endif
#ifdef HAS_ANISOTROPY_MAP
uniform sampler2D u_anisotropy_tex;             // RG direction, B strength
#endif

out vec4 frag_color;

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

// The shading frame in world space, facing the eye's side: the geometric
// normal `ng` (vertex normal, or the face normal without one), the tangent
// and bitangent (from TANGENT, else from UV derivatives — Mikkelsen's
// derivative frame — else any perpendicular pair), and `n`, the normal map
// applied through them.
struct ShadingFrame {
    vec3 t;
    vec3 b;
    vec3 n;
    vec3 ng;
};

ShadingFrame shading_frame() {
    ShadingFrame f;
#ifdef HAS_NORMAL
    f.ng = normalize(v_normal);
    if (!gl_FrontFacing) f.ng = -f.ng;
#else
    // Faces the eye whichever way the triangle winds.
    f.ng = normalize(cross(dFdx(v_world), dFdy(v_world)));
#endif
#if defined(HAS_NORMAL) && defined(HAS_TANGENT)
    f.t = normalize(v_tangent.xyz);
    f.b = cross(normalize(v_normal), f.t) * v_tangent.w;
    if (!gl_FrontFacing) { f.t = -f.t; f.b = -f.b; }
#elif defined(HAS_NORMAL_MAP) || defined(HAS_UV0)
#ifdef HAS_NORMAL_MAP
    vec2 uv = NORMAL_UV;
#else
    vec2 uv = v_uv0;
#endif
    vec3 dp1 = dFdx(v_world);
    vec3 dp2 = dFdy(v_world);
    vec2 duv1 = dFdx(uv);
    vec2 duv2 = dFdy(uv);
    vec3 dp2perp = cross(dp2, f.ng);
    vec3 dp1perp = cross(f.ng, dp1);
    f.t = dp2perp * duv1.x + dp1perp * duv2.x;
    f.b = dp2perp * duv1.y + dp1perp * duv2.y;
    float inv = inversesqrt(max(dot(f.t, f.t), dot(f.b, f.b)) + 1e-12);
    f.t *= inv;
    f.b *= inv;
    // UVs that don't vary across the pixel give no direction: any will do.
    if (dot(f.t, f.t) < 1e-8) {
        f.t = normalize(abs(f.ng.y) < 0.999 ? cross(vec3(0.0, 1.0, 0.0), f.ng)
                                            : cross(vec3(1.0, 0.0, 0.0), f.ng));
        f.b = cross(f.ng, f.t);
    }
#else
    f.t = normalize(abs(f.ng.y) < 0.999 ? cross(vec3(0.0, 1.0, 0.0), f.ng)
                                        : cross(vec3(1.0, 0.0, 0.0), f.ng));
    f.b = cross(f.ng, f.t);
#endif
    f.n = f.ng;
#ifdef HAS_NORMAL_MAP
    vec3 m = texture(u_normal_tex, NORMAL_UV).xyz * 2.0 - 1.0;
    m.xy *= u_normal_scale;
    f.n = normalize(mat3(f.t, f.b, f.ng) * m);
#endif
    return f;
}

void main() {
    peel_discard();
    vec4 base = base_color();
#ifdef ALPHAMODE_MASK
    if (base.a < u_alpha_cutoff) discard;
    base.a = 1.0;
#endif
#ifdef ALPHAMODE_OPAQUE
    base.a = 1.0;
#endif

#ifdef MATERIAL_UNLIT
    frag_color = vec4(scene_output_unlit(base.rgb), base.a);
#else
    Surface s;
    s.position = v_world;
    s.v = normalize(u_camera_pos.xyz - v_world);
    ShadingFrame frame = shading_frame();
    s.n = frame.n;
    s.n_coat = frame.ng;
    s.base_color = base.rgb;
    s.opacity = base.a;
    s.base_weight = u_base_weight;
    s.metallic = u_metallic_factor;
    float roughness = u_roughness_factor;
#ifdef HAS_METALLIC_ROUGHNESS_MAP
    vec4 mr = texture(u_metallic_roughness_tex, METALLIC_ROUGHNESS_UV);
    roughness *= mr.g;
    s.metallic *= mr.b;
#endif
    s.metallic = clamp(s.metallic, 0.0, 1.0);
    s.roughness = specular_aa(s.n_coat, clamp(roughness, 0.0, 1.0), u_shading.x, u_shading.y);
    s.diffuse_roughness = 0.0;
#ifdef HAS_DIFFUSE_ROUGHNESS
    s.diffuse_roughness = clamp(u_diffuse_roughness, 0.0, 1.0);
#endif
    s.ior = u_ior;
    s.specular_weight = u_specular_factor;
    s.specular_color = u_specular_color;
#ifdef HAS_SPECULAR_MAP
    s.specular_weight *= texture(u_specular_tex, SPECULAR_UV).a;
#endif
#ifdef HAS_SPECULAR_COLOR_MAP
    s.specular_color *= texture(u_specular_color_tex, SPECULAR_COLOR_UV).rgb;
#endif
    s.transmission = 0.0;
#ifdef HAS_TRANSMISSION
    s.transmission = u_transmission;
#ifdef HAS_TRANSMISSION_MAP
    s.transmission *= texture(u_transmission_tex, TRANSMISSION_UV).r;
#endif
#endif
    s.thickness = 0.0;
    s.attenuation_color = vec3(1.0);
    s.attenuation_distance = 0.0;
#ifdef HAS_VOLUME
    s.thickness = u_thickness;
#ifdef HAS_THICKNESS_MAP
    s.thickness *= texture(u_thickness_tex, THICKNESS_UV).g;
#endif
    s.attenuation_color = u_attenuation_color;
    s.attenuation_distance = u_attenuation_distance;
#endif
    s.dispersion = 0.0;
#ifdef HAS_DISPERSION
    s.dispersion = u_dispersion;
#endif
    s.coat = 0.0;
    s.coat_roughness = 0.0;
    s.coat_ior = 1.5;
    s.coat_color = vec3(1.0);
    s.coat_darkening = 0.0;
    s.coat_anisotropy = 0.0;
    s.coat_t = frame.t;
    s.coat_b = frame.b;
#ifdef HAS_CLEARCOAT
    s.coat = u_clearcoat;
    float coat_roughness = u_clearcoat_roughness;
#ifdef HAS_CLEARCOAT_MAP
    s.coat *= texture(u_clearcoat_tex, CLEARCOAT_UV).r;
#endif
#ifdef HAS_CLEARCOAT_ROUGHNESS_MAP
    coat_roughness *= texture(u_clearcoat_roughness_tex, CLEARCOAT_ROUGHNESS_UV).g;
#endif
#ifdef HAS_CLEARCOAT_NORMAL_MAP
    // The coat's own normal map, through the same frame.
    vec3 cm = texture(u_clearcoat_normal_tex, CLEARCOAT_NORMAL_UV).xyz * 2.0 - 1.0;
    cm.xy *= u_clearcoat_normal_scale;
    s.n_coat = normalize(mat3(frame.t, frame.b, frame.ng) * cm);
#endif
    s.coat_roughness = specular_aa(frame.ng, clamp(coat_roughness, 0.0, 1.0), u_shading.x, u_shading.y);
    s.coat_ior = u_coat_ior;
    s.coat_color = u_coat_color;
    s.coat_darkening = u_coat_darkening;
    // Along the surface tangent (OpenPBR geometry_coat_tangent's default),
    // in the coat's plane.
    s.coat_anisotropy = clamp(u_coat_anisotropy, 0.0, 1.0);
    s.coat_t = normalize(frame.t - s.n_coat * dot(s.n_coat, frame.t));
    s.coat_b = cross(s.n_coat, s.coat_t);
#endif
    s.subsurface = 0.0;
    s.subsurface_color = vec3(0.0);
    s.subsurface_mfp = vec3(1.0);
    s.subsurface_radius = 1.0;
#ifdef HAS_SUBSURFACE
    s.subsurface = clamp(u_subsurface_weight, 0.0, 1.0);
    s.subsurface_color = max(u_subsurface_color, vec3(0.0));
    s.subsurface_mfp = u_subsurface_radius * u_subsurface_radius_scale;
    // The curvature radius, from how fast the normal turns per unit of
    // surface (MaterialX mx_subsurface_scattering_approx).
    float curvature = length(fwidth(frame.ng)) / max(length(fwidth(v_world)), 1e-6);
    s.subsurface_radius = 1.0 / max(curvature, 0.01);
#endif
    s.ao = 1.0;
#ifdef HAS_OCCLUSION_MAP
    s.ao = 1.0 + u_occlusion_strength * (texture(u_occlusion_tex, OCCLUSION_UV).r - 1.0);
#endif
    s.sheen_color = vec3(0.0);
    s.sheen_roughness = 0.0;
#ifdef HAS_SHEEN
    s.sheen_color = u_sheen_color;
    float sheen_roughness = u_sheen_roughness;
#ifdef HAS_SHEEN_COLOR_MAP
    s.sheen_color *= texture(u_sheen_color_tex, SHEEN_COLOR_UV).rgb;
#endif
#ifdef HAS_SHEEN_ROUGHNESS_MAP
    sheen_roughness *= texture(u_sheen_roughness_tex, SHEEN_ROUGHNESS_UV).a;
#endif
    s.sheen_roughness = clamp(sheen_roughness, 0.0, 1.0);
#endif
    s.iridescence = 0.0;
    s.iridescence_ior = 1.3;
    s.iridescence_thickness = 0.0;
#ifdef HAS_IRIDESCENCE
    s.iridescence = u_iridescence;
#ifdef HAS_IRIDESCENCE_MAP
    s.iridescence *= texture(u_iridescence_tex, IRIDESCENCE_UV).r;
#endif
    s.iridescence_ior = u_iridescence_ior;
    s.iridescence_thickness = u_iridescence_thickness;
#ifdef HAS_IRIDESCENCE_THICKNESS_MAP
    s.iridescence_thickness = mix(u_iridescence_thickness_min, u_iridescence_thickness,
        texture(u_iridescence_thickness_tex, IRIDESCENCE_THICKNESS_UV).g);
#endif
#endif
    s.anisotropy = 0.0;
    s.anisotropy_t = frame.t;
    s.anisotropy_b = frame.b;
#ifdef HAS_ANISOTROPY
    // The direction: the tangent turned by the rotation, in the surface
    // (Khronos getAnisotropyInfo).
    vec2 dir = vec2(1.0, 0.0);
    float strength = u_anisotropy_strength;
#ifdef HAS_ANISOTROPY_MAP
    vec3 at = texture(u_anisotropy_tex, ANISOTROPY_UV).rgb;
    dir = at.rg * 2.0 - 1.0;
    strength *= at.b;
#endif
    float c = cos(u_anisotropy_rotation);
    float sn = sin(u_anisotropy_rotation);
    dir = mat2(c, sn, -sn, c) * dir;
    s.anisotropy_t = normalize(mat3(frame.t, frame.b, frame.n) * vec3(dir, 0.0));
    s.anisotropy_b = cross(frame.ng, s.anisotropy_t);
    s.anisotropy = clamp(strength, 0.0, 1.0);
#endif
    s.emission = u_emissive_factor * u_emissive_strength;
#ifdef HAS_EMISSIVE_MAP
    s.emission *= texture(u_emissive_tex, EMISSIVE_UV).rgb;
#endif

#ifdef MODEL_GLTF
    vec3 color = shade_gltf(s);
#else
    vec3 color = shade_openpbr(s);
#endif
    frag_color = vec4(scene_output(color), base.a);
#endif
}
