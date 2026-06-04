use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;

use rswebgl::context::{Context, ContextOptions};
use rswebgl::draw::{DrawCommand, DrawMode, IndexType};
use rswebgl::framebuffer::ClearMask;
use rswebgl::pass::{Batch, Pass};
use rswebgl::render_state::RenderState;
use rswebgl::texture::{TextureFormat, TextureMagFilter, TextureMinFilter, TextureTarget};
use rswebgl::uniform_values::UniformValues;
use rswebgl::vao_builder::{AttrKind, VaoBuilder};

const VERT: &str = r#"#version 300 es
precision highp float;
layout(location = 0) in vec2 a_pos;
layout(location = 1) in vec2 a_uv;
out vec2 v_uv;
void main() {
    gl_Position = vec4(a_pos, 0.0, 1.0);
    v_uv = a_uv;
}
"#;

const FRAG: &str = r#"#version 300 es
precision highp float;
in vec2 v_uv;
uniform sampler2D u_tex;
out vec4 frag_color;
void main() {
    frag_color = texture(u_tex, v_uv);
}
"#;

/// Build an 8×8 RGBA8 checkerboard (256 bytes) — two alternating colors.
fn checkerboard() -> Vec<u8> {
    let mut data = Vec::with_capacity(8 * 8 * 4);
    for y in 0..8 {
        for x in 0..8 {
            let on = (x + y) % 2 == 0;
            let (r, g, b) = if on { (230, 90, 60) } else { (40, 40, 50) };
            data.extend_from_slice(&[r, g, b, 255]);
        }
    }
    data
}

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
    let program = ctx.create_program(VERT, FRAG).expect("program");

    // Fullscreen quad: position (clip space) + uv.
    #[rustfmt::skip]
    let positions: [f32; 8] = [
        -1.0, -1.0,
         1.0, -1.0,
         1.0,  1.0,
        -1.0,  1.0,
    ];
    #[rustfmt::skip]
    let uvs: [f32; 8] = [
        0.0, 0.0,
        1.0, 0.0,
        1.0, 1.0,
        0.0, 1.0,
    ];
    let indices: [u16; 6] = [0, 1, 2, 0, 2, 3];

    let mut vao_builder = VaoBuilder::new(&ctx);
    vao_builder.add_f32(0, AttrKind::F32x2, &positions);
    vao_builder.add_f32(1, AttrKind::F32x2, &uvs);
    vao_builder.add_indices_u16(&indices);
    let vao = vao_builder.build().expect("vao");

    // Storage-first texture: allocate one immutable level, then fill it.
    // NEAREST so the 8×8 checkerboard stays crisp when stretched.
    let tex = ctx
        .create_texture(
            TextureTarget::Texture2D,
            TextureMinFilter::Nearest,
            TextureMagFilter::Nearest,
        )
        .expect("texture");
    tex.storage_2d(1, &TextureFormat::rgba8(), 8, 8);
    tex.sub_image_2d(0, 0, 0, 8, 8, &TextureFormat::rgba8(), &checkerboard());

    // (DOM path, for reference — loads asynchronously and uploads on `onload`:)
    //   let cb = Closure::<dyn FnMut()>::new(|| {}).into_js_value();
    //   tex.load_image("./photo.png", &TextureFormat::rgba8(), true, true, cb.into());

    let mut uniforms = UniformValues::new();
    uniforms.set_sampler("u_tex", &tex);

    let render_state = RenderState::new();

    let renderer = ctx.renderer();
    let default_fb = ctx.default_framebuffer();
    default_fb.set_clear_color(0.05, 0.05, 0.08, 1.0);
    default_fb.enable_auto_resize().expect("auto-resize");

    let draw_cmd = DrawCommand::elements(DrawMode::Triangles, 6, IndexType::UnsignedShort, 0);

    let mut pass = Pass::to_default();
    pass.set_clear(ClearMask::color());
    let mut batch = Batch::new(&program, &render_state);
    batch.draw(&vao, &uniforms, draw_cmd);
    pass.add(batch);

    // Re-render each frame so the quad tracks the auto-resized buffer.
    let f: Rc<RefCell<Option<Closure<dyn FnMut(f64)>>>> = Rc::new(RefCell::new(None));
    let g = f.clone();
    let win = window.clone();
    *g.borrow_mut() = Some(Closure::<dyn FnMut(f64)>::new(move |_time: f64| {
        renderer.render(&pass);
        let _ = win.request_animation_frame(f.borrow().as_ref().unwrap().as_ref().unchecked_ref());
    }));
    let _ = window.request_animation_frame(g.borrow().as_ref().unwrap().as_ref().unchecked_ref());
}
