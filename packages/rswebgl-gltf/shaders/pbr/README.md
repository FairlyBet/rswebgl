# glTF PBR reference shader

`main.vert` / `main.frag` declare the **full data interface** a glTF 2.0
metallic-roughness primitive + material can drive (per the spec), with every
optional input gated behind a feature `#define`. The lighting math in
`main.frag` is a documented **placeholder** (`shade()` / `// TODO(pbr)`): the
geometry and material data are fully plumbed, but the Cook-Torrance BRDF + IBL
are still to come.

## Using it

The shaders compile as plain WebGL2 GLSL ES 3.00. The host enables features by
injecting `#define` lines **immediately after the `#version 300 es` line** (one
per attribute/texture/feature the primitive+material actually provide). The full
list lives in the header comment of each file; `vert` and `frag` must agree on
the geometry defines.

## Host-side contract

Attribute locations (already emitted by `src/geometry.rs`):

| loc | attribute   | type  | define                       |
|-----|-------------|-------|------------------------------|
| 0   | POSITION    | vec3  | (always)                     |
| 1   | NORMAL      | vec3  | `HAS_NORMAL_VEC3`            |
| 2   | TEXCOORD_0  | vec2  | `HAS_TEXCOORD_0_VEC2`        |
| 3   | TANGENT     | vec4  | `HAS_TANGENT_VEC4`           |
| 4   | COLOR_0     | vec3/4| `HAS_COLOR_0_VEC3`/`_VEC4`   |
| 5   | TEXCOORD_1  | vec2  | `HAS_TEXCOORD_1_VEC2`        |
| 6   | JOINTS_0    | uvec4 | `USE_SKINNING`               |
| 7   | WEIGHTS_0   | vec4  | `USE_SKINNING`               |

std140 uniform blocks (field order = the `UboLayout` the host must build):

- **`Frame`** — `mat4 u_view_proj`, `vec4 u_camera_position`
- **`Object`** — `mat4 u_model`, `mat4 u_normal_matrix` (inverse-transpose of
  model; mat4 to dodge std140 mat3 padding)
- **`Material`** — `vec4 u_base_color_factor`, `vec4 u_emissive_factor`
  (`.rgb` factor, `.w` strength), `float u_metallic_factor`,
  `float u_roughness_factor`, `float u_normal_scale`,
  `float u_occlusion_strength`, `float u_alpha_cutoff`
- **`Skin`** (`USE_SKINNING`) — `mat4 u_joint_matrix[MAX_JOINTS]` (default 64)
- **`Lights`** (`USE_PUNCTUAL`) — `Light u_lights[MAX_LIGHTS]` + `int u_light_count`

Sampler uniforms (assign a texture unit via `uniform1i`): `u_base_color_texture`,
`u_metallic_roughness_texture`, `u_normal_texture`, `u_occlusion_texture`,
`u_emissive_texture`, and for `USE_IBL`: `u_ibl_irradiance`,
`u_ibl_prefiltered`, `u_ibl_brdf_lut`. Each map's TEXCOORD set defaults to UV0;
override with e.g. `#define BASE_COLOR_UV v_uv1`.

Color targets: location 0 `g_final_color` always; optional MRT outputs
`g_emissive_color` (`OUTPUT_EMISSIVE_TARGET`) and `g_normal_color`
(`OUTPUT_NORMAL_TARGET`).

> The current translator (`DrawItem::write_uniforms`) emits a smaller, flat-
> uniform convention used by `examples/gltf`. Wiring these UBO blocks + defines
> into the translator (a material→shader permutation system) is deferred — see
> `TODO.md`.
