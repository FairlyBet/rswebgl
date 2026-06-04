// Render a cubemap, then display it rotating.
//
// Two phases:
//   1. Bake — render a distinct procedural pattern into each of the six faces of
//      a cube-map texture, one FBO pass per face (offline, once at startup).
//   2. Display — every frame, draw a fullscreen triangle and sample the baked
//      cubemap along a per-pixel view ray that we rotate over time, i.e. a skybox
//      with a slowly panning camera.

use std::cell::RefCell;
use std::rc::Rc;

use glam::Mat3;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;

use rswebgl::context::{Context, ContextOptions};
use rswebgl::draw::{DrawCommand, DrawMode};
use rswebgl::pass::{Batch, Pass};
use rswebgl::render_state::RenderState;
use rswebgl::texture::{
    CubeMapFace, TextureFormat, TextureMagFilter, TextureMinFilter, TextureTarget,
};
use rswebgl::uniform_values::UniformValues;

const CUBE_SIZE: i32 = 512;
const FOV_DEG: f32 = 60.0;

// Fullscreen triangle from gl_VertexID — no VAO, no vertex buffer. The three
// vertices (uv 0,0 / 2,0 / 0,2) map to clip positions (-1,-1)/(3,-1)/(-1,3),
// which cover the whole viewport; the visible square sees uv in [0, 1].
const FULLSCREEN_VERT: &str = r#"#version 300 es
precision highp float;
out vec2 v_uv;
void main() {
    vec2 uv = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
    v_uv = uv;
    gl_Position = vec4(uv * 2.0 - 1.0, 0.0, 1.0);
}
"#;

// Bake shader: a unique color per face plus a grid, a vertical gradient, and a
// border — enough structure to read which face you're looking at and to make the
// rotation obvious.
const BAKE_FRAG: &str = r#"#version 300 es
precision highp float;
in vec2 v_uv;
uniform int u_face;
out vec4 frag_color;
void main() {
    vec3 cols[6] = vec3[6](
        vec3(0.85, 0.25, 0.25), // +X  red
        vec3(0.25, 0.55, 0.85), // -X  blue
        vec3(0.30, 0.80, 0.35), // +Y  green
        vec3(0.85, 0.75, 0.25), // -Y  yellow
        vec3(0.65, 0.35, 0.80), // +Z  purple
        vec3(0.90, 0.55, 0.25)  // -Z  orange
    );
    vec3 base = cols[u_face];

    // 8x8 grid lines.
    vec2 g = abs(fract(v_uv * 8.0) - 0.5);
    float line = smoothstep(0.0, 0.03, min(g.x, g.y));

    // Vertical gradient + darkened grid lines.
    vec3 c = base * mix(0.55, 1.0, v_uv.y);
    c = mix(c * 0.3, c, line);

    // White border so face edges are visible in the skybox.
    vec2 b = step(0.02, v_uv) * step(0.02, 1.0 - v_uv);
    c = mix(vec3(1.0), c, b.x * b.y);

    frag_color = vec4(c, 1.0);
}
"#;

// Skybox vertex shader: reconstruct a view ray per vertex and rotate it into
// world space. The camera looks down -Z; `u_rot` pans it over time.
const SKY_VERT: &str = r#"#version 300 es
precision highp float;
uniform mat3 u_rot;
uniform float u_aspect;
uniform float u_tan_half_fov;
out vec3 v_dir;
void main() {
    vec2 uv = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
    vec2 ndc = uv * 2.0 - 1.0;
    vec3 ray = vec3(ndc.x * u_aspect * u_tan_half_fov, ndc.y * u_tan_half_fov, -1.0);
    v_dir = u_rot * ray;
    // z = 1.0 keeps the skybox at the far plane (harmless here: depth test off).
    gl_Position = vec4(ndc, 1.0, 1.0);
}
"#;

const SKY_FRAG: &str = r#"#version 300 es
precision highp float;
in vec3 v_dir;
uniform samplerCube u_sky;
out vec4 frag_color;
void main() {
    frag_color = texture(u_sky, normalize(v_dir));
}
"#;

fn main() {
    let window = web_sys::window().expect("no window");
    let document = window.document().expect("no document");
    let canvas: HtmlCanvasElement = document
        .get_element_by_id("c")
        .expect("missing #c canvas")
        .dyn_into()
        .expect("not a canvas");

    let options = ContextOptions::new();
    let ctx = Context::from_canvas(&canvas, &options).expect("ctx");

    let renderer = ctx.renderer();
    let render_state = RenderState::new(); // depth test & culling off — fine for fullscreen passes

    // ---- the cubemap we render into, then sample -------------------------
    // Linear filtering, single mip level; wrap defaults to CLAMP_TO_EDGE which is
    // what you want for a cubemap. storage_2d on a cube target allocates all six
    // faces at once.
    let cube = ctx
        .create_texture(
            TextureTarget::TextureCubeMap,
            TextureMinFilter::Linear,
            TextureMagFilter::Linear,
        )
        .expect("cube texture");
    cube.storage_2d(1, &TextureFormat::rgba8(), CUBE_SIZE, CUBE_SIZE);

    // ---- phase 1: bake each face via an FBO ------------------------------
    let bake_program = ctx
        .create_program(FULLSCREEN_VERT, BAKE_FRAG)
        .expect("bake program");
    let fbo = ctx
        .create_framebuffer(CUBE_SIZE, CUBE_SIZE)
        .expect("framebuffer");

    let faces = [
        CubeMapFace::PositiveX,
        CubeMapFace::NegativeX,
        CubeMapFace::PositiveY,
        CubeMapFace::NegativeY,
        CubeMapFace::PositiveZ,
        CubeMapFace::NegativeZ,
    ];

    let tri = DrawCommand::arrays(DrawMode::Triangles, 0, 3);
    for (i, face) in faces.iter().enumerate() {
        // Point the FBO's color attachment at this face, then render into it. The
        // fullscreen triangle covers the whole face, so no clear is needed.
        fbo.attach_color_cube_face(0, &cube, face.clone(), 0);
        if i == 0 && !renderer.is_framebuffer_complete(&fbo) {
            web_sys::console::error_1(&"cubemap FBO incomplete".into());
        }

        let uniforms = UniformValues::new();
        uniforms.set_int("u_face", i as i32);

        let mut pass = Pass::new(&fbo);
        let mut batch = Batch::new(&bake_program, &render_state);
        batch.draw_vertexless(&uniforms, tri);
        pass.add(batch);
        renderer.render(&pass);
    }

    // ---- phase 2: display the cubemap as a rotating skybox ---------------
    let sky_program = ctx.create_program(SKY_VERT, SKY_FRAG).expect("sky program");
    let default_fb = ctx.default_framebuffer();
    default_fb.enable_auto_resize().expect("auto-resize");

    let sky_uniforms = UniformValues::new();
    sky_uniforms.set_sampler("u_sky", &cube);
    sky_uniforms.set_float("u_tan_half_fov", (FOV_DEG.to_radians() * 0.5).tan());

    // requestAnimationFrame loop with self-reference via Rc<RefCell<Option<..>>>.
    let f: Rc<RefCell<Option<Closure<dyn FnMut(f64)>>>> = Rc::new(RefCell::new(None));
    let g = f.clone();
    let win = window.clone();

    *g.borrow_mut() = Some(Closure::<dyn FnMut(f64)>::new(move |time: f64| {
        let t = time as f32 / 1000.0;

        // Pan the camera: continuous yaw, gentle bobbing pitch.
        let rot = Mat3::from_rotation_y(t * 0.3) * Mat3::from_rotation_x(0.2 * (t * 0.5).sin());
        sky_uniforms.set_mat3("u_rot", false, &rot.to_cols_array());

        let aspect = default_fb.width().max(1) as f32 / default_fb.height().max(1) as f32;
        sky_uniforms.set_float("u_aspect", aspect);

        let mut pass = Pass::to_default();
        let mut batch = Batch::new(&sky_program, &render_state);
        batch.draw_vertexless(&sky_uniforms, tri);
        pass.add(batch);
        renderer.render(&pass);

        let _ = win.request_animation_frame(f.borrow().as_ref().unwrap().as_ref().unchecked_ref());
    }));

    let _ = window.request_animation_frame(g.borrow().as_ref().unwrap().as_ref().unchecked_ref());
}
