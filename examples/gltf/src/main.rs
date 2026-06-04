use std::cell::RefCell;
use std::rc::Rc;

use glam::{Mat4, Vec3};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;

use rswebgl::buffer::BufferUsage;
use rswebgl::console;
use rswebgl::context::{Context, ContextOptions};
use rswebgl::framebuffer::ClearMask;
use rswebgl::pass::{Batch, Pass};
use rswebgl::uniform_buffer::{Std140Type, UboLayout};
use rswebgl::uniform_values::UniformValues;
use rswebgl_gltf::Model;

// A small, self-contained Khronos sample (GLB: geometry + textures embedded).
// Served with permissive CORS, so it loads cross-origin without a local copy.
// Drop your own `model.glb` next to index.html and change this to "./model.glb".
const MODEL_URL: &str = "https://raw.githubusercontent.com/KhronosGroup/glTF-Sample-Models/main/2.0/Avocado/glTF-Binary/Avocado.glb";

// Geometry uses the translator's fixed attribute locations (0=pos, 1=normal,
// 2=uv). The MVP and model matrices come from a std140 uniform block; material
// values come from `DrawItem::write_uniforms` (the documented convention).
const VERT: &str = r#"#version 300 es
precision highp float;
layout(location = 0) in vec3 a_pos;
layout(location = 1) in vec3 a_normal;
layout(location = 2) in vec2 a_uv;
uniform Matrices {
    mat4 u_mvp;
    mat4 u_model;
};
out vec3 v_normal;
out vec2 v_uv;
void main() {
    gl_Position = u_mvp * vec4(a_pos, 1.0);
    v_normal = mat3(u_model) * a_normal;
    v_uv = a_uv;
}
"#;

const FRAG: &str = r#"#version 300 es
precision highp float;
in vec3 v_normal;
in vec2 v_uv;
uniform vec4 u_base_color_factor;
uniform bool u_has_base_color_tex;
uniform sampler2D u_base_color_tex;
uniform float u_alpha_cutoff;
uniform int u_alpha_mode;
out vec4 frag_color;
void main() {
    vec4 base = u_base_color_factor;
    if (u_has_base_color_tex) {
        base *= texture(u_base_color_tex, v_uv);
    }
    if (u_alpha_mode == 1 && base.a < u_alpha_cutoff) {
        discard; // MASK
    }
    vec3 n = normalize(v_normal);
    if (!gl_FrontFacing) {
        n = -n; // double-sided lighting
    }
    vec3 l = normalize(vec3(0.4, 0.8, 0.6));
    float ndl = max(dot(n, l), 0.0);
    vec3 color = base.rgb * (0.25 + 0.75 * ndl); // ambient + diffuse
    frag_color = vec4(color, base.a);
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

    let mut options = ContextOptions::new();
    options.antialias = true;
    options.depth = true;
    let ctx = Context::from_canvas(&canvas, &options).expect("ctx");

    // Loading is async (it fetches the GLB). Own the Context inside the future;
    // the render loop set up afterwards keeps GL alive via cloned handles, so
    // the Context can drop once the loop is wired.
    wasm_bindgen_futures::spawn_local(async move {
        match rswebgl_gltf::load_model(&ctx, MODEL_URL).await {
            Ok(model) => run(ctx, model),
            Err(e) => console::error(&format!("[example-gltf] load failed: {e}")),
        }
    });
}

/// One renderable item: its spinning UBO and static world matrix.
struct Item {
    ubo: rswebgl::uniform_buffer::UniformBuffer,
    world: Mat4,
}

fn run(ctx: Context, model: Model) {
    let program = ctx.create_program(VERT, FRAG).expect("program");
    let renderer = ctx.renderer();
    let default_fb = ctx.default_framebuffer();
    default_fb.set_clear_color(0.05, 0.05, 0.08, 1.0);
    default_fb.enable_auto_resize().expect("auto-resize");

    // Frame the camera from the model's world-space bounding sphere.
    let c = model.center();
    let center = Vec3::new(c[0], c[1], c[2]);
    let radius = model.radius();

    // The block holds two mat4s (MVP + model), updated per frame and bound by
    // block name. Each draw item gets its own UBO + uniforms so it carries its
    // own material and transform.
    let layout = UboLayout::new()
        .field("u_mvp", Std140Type::Mat4)
        .field("u_model", Std140Type::Mat4);

    let mut pass = Pass::to_default();
    pass.set_clear(ClearMask::color_depth());
    let mut items: Vec<Item> = Vec::new();

    for i in 0..model.draw_item_count() {
        let di = model.draw_item(i).expect("draw item");
        let ubo = ctx
            .create_uniform_buffer(&layout, BufferUsage::DynamicDraw)
            .expect("ubo");
        let uniforms = UniformValues::new();
        uniforms.set_uniform_block("Matrices", &ubo);
        di.write_uniforms(&model, &uniforms);

        let mut batch = Batch::new(&program, &di.render_state());
        batch.draw(&di.vao(), &uniforms, di.draw_command());
        pass.add(batch);

        items.push(Item {
            ubo,
            world: Mat4::from_cols_array(&di.model_cols()),
        });
    }

    if items.is_empty() {
        console::warn("[example-gltf] model has no drawable primitives");
        return;
    }

    // Static camera looking at the model center; the model spins around +Y.
    let fov = 45f32.to_radians();
    let dist = radius / (fov * 0.5).sin() * 1.4;
    let eye = center + Vec3::new(0.0, radius * 0.35, dist);
    let view = Mat4::look_at_rh(eye, center, Vec3::Y);
    let near = (dist - radius * 2.0).max(radius * 0.01).max(1e-4);
    let far = dist + radius * 2.0;

    let f: Rc<RefCell<Option<Closure<dyn FnMut(f64)>>>> = Rc::new(RefCell::new(None));
    let g = f.clone();
    let win = web_sys::window().expect("no window");

    *g.borrow_mut() = Some(Closure::<dyn FnMut(f64)>::new(move |time: f64| {
        let t = time as f32 / 1000.0;
        let aspect = default_fb.width().max(1) as f32 / default_fb.height().max(1) as f32;
        let proj = Mat4::perspective_rh_gl(fov, aspect, near, far);
        // Spin the model about its own center.
        let spin = Mat4::from_translation(center)
            * Mat4::from_rotation_y(t * 0.6)
            * Mat4::from_translation(-center);

        for item in &items {
            let model_mat = spin * item.world;
            let mvp = proj * view * model_mat;
            item.ubo.set_mat("u_mvp", &mvp.to_cols_array());
            item.ubo.set_mat("u_model", &model_mat.to_cols_array());
        }

        renderer.render(&pass);
        let _ = win.request_animation_frame(f.borrow().as_ref().unwrap().as_ref().unchecked_ref());
    }));

    let _ = web_sys::window()
        .expect("no window")
        .request_animation_frame(g.borrow().as_ref().unwrap().as_ref().unchecked_ref());
}
