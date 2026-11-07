//! Shaders for the scene: a prelude per stage declares everything the scene
//! binds — the camera (`Frame` block), lights and environment (`Lighting`
//! block, cube maps, BRDF table), the object's matrices — plus helpers to
//! light with them and to output a pixel. A scene shader is just its own
//! code on top; it can't get the declarations wrong, and the built-in PBR
//! uses the same preludes.
//!
//! Vertex prelude: `u_view`, `u_proj`, `u_view_proj`, `u_camera_pos`,
//! `u_model`, `u_normal_matrix`.
//! Fragment prelude: the same, plus `u_lights[]`, `u_environment`,
//! `u_lighting`, `u_env_diffuse`, `u_env_specular`, `u_env_brdf`, and
//! `scene_irradiance(n)`, `scene_reflection(r, roughness)`,
//! `scene_output(linear)` (exposure, tone mapping, sRGB), `tone_map`,
//! `to_srgb`, `env_dir`.
//!
//! ```glsl
//! // vertex
//! layout(location = 0) in vec3 a_position;
//! layout(location = 1) in vec3 a_normal;
//! out vec3 v_normal;
//! void main() {
//!     v_normal = u_normal_matrix * a_normal;
//!     gl_Position = u_view_proj * u_model * vec4(a_position, 1.0);
//! }
//! // fragment
//! in vec3 v_normal;
//! uniform vec3 u_color; // in the mesh's material group
//! out vec4 frag_color;
//! void main() {
//!     frag_color = vec4(scene_output(u_color * scene_irradiance(normalize(v_normal))), 1.0);
//! }
//! ```

use rswebgl::context::Context;
use rswebgl::error::Error;
use rswebgl::ids::ProgramId;

/// The vertex-stage prelude (`shaders/scene.vert.glsl`).
pub const VERT_PRELUDE: &str = include_str!("../shaders/scene.vert.glsl");
/// The fragment-stage prelude (`shaders/scene.frag.glsl`).
pub const FRAG_PRELUDE: &str = include_str!("../shaders/scene.frag.glsl");

/// Complete sources: `#version 300 es`, `defines` (whole `#define` lines),
/// the prelude, then your code — numbered from line 1 again, so compile
/// errors point into your code, not the prelude.
pub fn shader_sources(vert: &str, frag: &str, defines: &str) -> (String, String) {
    let stage =
        |prelude: &str, code: &str| format!("#version 300 es\n{defines}{prelude}\n#line 1\n{code}");
    (stage(VERT_PRELUDE, vert), stage(FRAG_PRELUDE, frag))
}

/// Compiles a scene shader (see the module docs): your vertex and fragment
/// code without `#version` or the scene's declarations. The program is
/// yours to destroy.
pub fn create_scene_program(ctx: &Context, vert: &str, frag: &str) -> Result<ProgramId, Error> {
    let (v, f) = shader_sources(vert, frag, "");
    ctx.create_program(&v, &f)
}
