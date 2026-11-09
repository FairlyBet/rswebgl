use std::cell::RefCell;
use std::rc::Rc;

use glam::{Mat4, Vec3};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{HtmlCanvasElement, KeyboardEvent};

use rswebgl::buffer::BufferUsage;
use rswebgl::context::{Context, ContextOptions};
use rswebgl::draw::{DrawCommand, DrawMode, IndexType};
use rswebgl::framebuffer::ClearMask;
use rswebgl::ids::UniformBufferId;
use rswebgl::param::ParamLayoutBuilder;
use rswebgl::pass::{Batch, Pass};
use rswebgl::render_state::{DepthFunc, RenderState};
use rswebgl::uniform_buffer::{UboLayout, UniformType};
use rswebgl::vao_builder::{AttrKind, VaoBuilder};

// The MVP comes from a uniform block (UBO) instead of a plain uniform. The block
// name "Matrices" is what binds it on the Rust side; the member is std140-packed.
const VERT: &str = r#"#version 300 es
precision highp float;
layout(location = 0) in vec3 a_pos;
layout(location = 1) in vec3 a_color;
uniform Matrices {
    mat4 u_mvp;
};
out vec3 v_color;
void main() {
    gl_Position = u_mvp * vec4(a_pos, 1.0);
    v_color = a_color;
}
"#;

const FRAG: &str = r#"#version 300 es
precision highp float;
in vec3 v_color;
out vec4 frag_color;
void main() {
    frag_color = vec4(v_color, 1.0);
}
"#;

// Everything that lives on the GPU. Built once at startup and again after a
// context restore: handles from before a context loss are dead.
struct Scene {
    pass: Pass,
    ubo: UniformBufferId,
}

fn build_scene(ctx: &Context) -> Result<Scene, String> {
    let program = ctx.create_program(VERT, FRAG)?;

    #[rustfmt::skip]
    let positions: [f32; 24] = [
        -0.5, -0.5, -0.5,
         0.5, -0.5, -0.5,
         0.5,  0.5, -0.5,
        -0.5,  0.5, -0.5,
        -0.5, -0.5,  0.5,
         0.5, -0.5,  0.5,
         0.5,  0.5,  0.5,
        -0.5,  0.5,  0.5,
    ];

    #[rustfmt::skip]
    let colors: [f32; 24] = [
        0.0, 0.0, 0.0,
        1.0, 0.0, 0.0,
        1.0, 1.0, 0.0,
        0.0, 1.0, 0.0,
        0.0, 0.0, 1.0,
        1.0, 0.0, 1.0,
        1.0, 1.0, 1.0,
        0.0, 1.0, 1.0,
    ];

    #[rustfmt::skip]
    let indices: [u16; 36] = [
        0,2,1, 0,3,2, // back  (-z)
        4,5,6, 4,6,7, // front (+z)
        0,4,7, 0,7,3, // left  (-x)
        1,2,6, 1,6,5, // right (+x)
        0,1,5, 0,5,4, // bottom(-y)
        3,7,6, 3,6,2, // top   (+y)
    ];

    let mut vao_builder = VaoBuilder::new(ctx);
    vao_builder.add_f32(0, AttrKind::F32x3, &positions);
    vao_builder.add_f32(1, AttrKind::F32x3, &colors);
    vao_builder.add_indices_u16(&indices);
    let vao = vao_builder.build()?;

    let mut render_state = RenderState::new();
    render_state.depth_test = true;
    render_state.depth_func = DepthFunc::Less;
    let render_state = ctx.render_state(&render_state);

    let draw_cmd = DrawCommand::elements(DrawMode::Triangles, 36, IndexType::UnsignedShort, 0);

    // One mat4 in a std140 block, updated each frame and bound by block name.
    let layout = UboLayout::new().field("u_mvp", UniformType::Mat4);
    let ubo = ctx.create_uniform_buffer(&layout, BufferUsage::DynamicDraw)?;
    // The block reaches the draw through a parameter group in slot 0.
    let params_layout = ctx.create_param_layout(&ParamLayoutBuilder::new().block("Matrices"))?;
    let params = ctx.create_param_group(params_layout)?;
    ctx.param_set_block_by_name(params, "Matrices", ubo);

    // One pass to the canvas, clearing first, with a single batch (program +
    // state) holding one draw.
    let mut pass = Pass::to_default();
    pass.set_clear(&ClearMask::color_depth());
    let mut batch = Batch::new(program, render_state);
    batch.set_group(0, params);
    batch.draw(vao, &draw_cmd);
    pass.add(&batch);

    Ok(Scene { pass, ubo })
}

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
    let scene = Rc::new(RefCell::new(build_scene(&ctx).expect("scene")));

    // On restore, rebuild every GPU resource: handles from before the loss are
    // stale. The callback gets the context as an argument.
    let restore_scene = Rc::clone(&scene);
    ctx.on_restored(move |ctx| match build_scene(ctx) {
        Ok(s) => *restore_scene.borrow_mut() = s,
        Err(e) => web_sys::console::error_1(&format!("rebuild failed: {e}").into()),
    });

    // Press L to simulate a context loss, and L again to restore it.
    let key_ctx = ctx.clone();
    let on_key = Closure::<dyn FnMut(KeyboardEvent)>::new(move |e: KeyboardEvent| {
        if e.key() == "l" || e.key() == "L" {
            if key_ctx.is_lost() {
                key_ctx.simulate_context_restore();
            } else {
                key_ctx.simulate_context_loss();
            }
        }
    });
    window
        .add_event_listener_with_callback("keydown", on_key.as_ref().unchecked_ref())
        .expect("keydown listener");
    on_key.forget();

    let default_fb = ctx.default_framebuffer();
    default_fb.set_clear_color(0.05, 0.05, 0.08, 1.0);
    // Track the canvas's CSS size × devicePixelRatio: keeps the drawing buffer and
    // viewport correct as the fullscreen canvas resizes.
    default_fb.enable_auto_resize().expect("auto-resize");

    let view = Mat4::from_translation(Vec3::new(0.0, 0.0, -3.0));

    // requestAnimationFrame loop with self-reference via Rc<RefCell<Option<Closure>>>.
    let f: FrameCallback = Rc::new(RefCell::new(None));
    let g = f.clone();
    let win = window.clone();

    *g.borrow_mut() = Some(Closure::<dyn FnMut(f64)>::new(move |time: f64| {
        let t = time as f32 / 1000.0;
        let model = Mat4::from_rotation_y(t) * Mat4::from_rotation_x(t * 0.7);
        // Recompute aspect each frame from the current (auto-resized) buffer size.
        let aspect = default_fb.width().max(1) as f32 / default_fb.height().max(1) as f32;
        let proj = Mat4::perspective_rh_gl(45f32.to_radians(), aspect, 0.1, 100.0);
        let mvp = proj * view * model;

        let scene = scene.borrow();
        // Write into the block's staging buffer; the renderer flushes it (one
        // bufferSubData) and binds the block when the pass draws.
        ctx.ubo_set_mat(scene.ubo, "u_mvp", &mvp.to_cols_array());
        // A no-op while the context is lost.
        ctx.render(&scene.pass);

        let _ = win.request_animation_frame(f.borrow().as_ref().unwrap().as_ref().unchecked_ref());
    }));

    let _ = window.request_animation_frame(g.borrow().as_ref().unwrap().as_ref().unchecked_ref());
}

// The requestAnimationFrame callback, holding itself to schedule the next frame.
type FrameCallback = Rc<RefCell<Option<Closure<dyn FnMut(f64)>>>>;
