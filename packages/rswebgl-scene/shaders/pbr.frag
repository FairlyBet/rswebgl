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
//              HAS_DIFFUSE_ROUGHNESS — a layer the material doesn't use isn't
//              compiled at all

#include "brdf.glsl"
#include "eon.glsl"
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
#endif
#ifdef HAS_DISPERSION
uniform float u_dispersion;
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
    frag_color = vec4(scene_output_unlit(base.rgb), base.a);
#else
    Surface s;
    s.position = v_world;
    s.v = normalize(u_camera_pos.xyz - v_world);
    s.n = shading_normal();
#ifdef HAS_NORMAL
    s.n_coat = normalize(v_normal);
#else
    s.n_coat = normalize(cross(dFdx(v_world), dFdy(v_world)));
#endif
    if (!gl_FrontFacing) s.n_coat = -s.n_coat;
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
    s.roughness = specular_aa(s.n_coat, clamp(roughness, 0.0, 1.0));
    s.diffuse_roughness = 0.0;
#ifdef HAS_DIFFUSE_ROUGHNESS
    s.diffuse_roughness = clamp(u_diffuse_roughness, 0.0, 1.0);
#endif
    s.ior = u_ior;
    s.specular_weight = u_specular_factor;
    s.specular_color = u_specular_color;
    s.transmission = 0.0;
#ifdef HAS_TRANSMISSION
    s.transmission = u_transmission;
#endif
    s.thickness = 0.0;
    s.attenuation_color = vec3(1.0);
    s.attenuation_distance = 0.0;
#ifdef HAS_VOLUME
    s.thickness = u_thickness;
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
#ifdef HAS_CLEARCOAT
    s.coat = u_clearcoat;
    s.coat_roughness = specular_aa(s.n_coat, clamp(u_clearcoat_roughness, 0.0, 1.0));
    s.coat_ior = u_coat_ior;
    s.coat_color = u_coat_color;
    s.coat_darkening = u_coat_darkening;
#endif
    s.ao = 1.0;
#ifdef HAS_OCCLUSION_MAP
    s.ao = 1.0 + u_occlusion_strength * (texture(u_occlusion_tex, OCCLUSION_UV).r - 1.0);
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
