// SceneRenderer against a real context: draws, frame block, object matrices,
// culling, visibility, cleanup. Run with:
// wasm-pack test --headless --chrome packages/rswebgl-scene
#![cfg(target_arch = "wasm32")]

use glam::Vec3;
use wasm_bindgen::JsCast;
use wasm_bindgen_test::*;
use web_sys::{HtmlCanvasElement, WebGl2RenderingContext as Gl};

use rswebgl::context::{Context, ContextOptions};
use rswebgl::draw::{DrawCommand, DrawMode};
use rswebgl::framebuffer::ClearMask;
use rswebgl::ids::ParamGroupId;
use rswebgl::param::ParamLayoutBuilder;
use rswebgl::pass::Pass;
use rswebgl::render_state::RenderState;
use rswebgl::uniform_buffer::UniformType;
use rswebgl::vao_builder::{AttrKind, VaoBuilder};
use rswebgl_ecs::{Entity, World};
use rswebgl_scene::{
    Bounds, Camera, MeshRenderer, RenderStats, SceneRenderer, Transform, TransformSystem, Visible,
    set_parent,
};

wasm_bindgen_test_configure!(run_in_browser);

const SIZE: i32 = 64;

const VERT: &str = r#"#version 300 es
precision highp float;
layout(location = 0) in vec3 a_pos;
layout(std140) uniform Frame {
    mat4 u_view;
    mat4 u_proj;
    mat4 u_view_proj;
    vec4 u_camera_pos;
};
uniform mat4 u_model;
void main() {
    gl_Position = u_view_proj * u_model * vec4(a_pos, 1.0);
}
"#;

const FRAG: &str = r#"#version 300 es
precision highp float;
uniform vec4 u_color;
out vec4 frag_color;
void main() { frag_color = u_color; }
"#;

const BLACK: [u8; 4] = [0, 0, 0, 255];
const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];

struct Setup {
    ctx: Context,
    gl: Gl,
    world: World,
    transforms: TransformSystem,
    scene: SceneRenderer,
    pass: Pass,
    camera: Entity,
    red: MeshRenderer,
    green: MeshRenderer,
}

fn setup() -> Setup {
    let canvas: HtmlCanvasElement = web_sys::window()
        .unwrap()
        .document()
        .unwrap()
        .create_element("canvas")
        .unwrap()
        .dyn_into()
        .unwrap();
    canvas.set_width(SIZE as u32);
    canvas.set_height(SIZE as u32);
    let mut options = ContextOptions::new();
    options.antialias = false;
    let ctx = Context::from_canvas(&canvas, &options).expect("ctx");
    let gl: Gl = canvas
        .get_context("webgl2")
        .unwrap()
        .unwrap()
        .dyn_into()
        .unwrap();

    let program = ctx.create_program(VERT, FRAG).expect("program");
    let state = ctx.render_state(&RenderState::new());
    // A unit quad around the origin in XY.
    let mut vb = VaoBuilder::new(&ctx);
    vb.add_f32(
        0,
        AttrKind::F32x3,
        &[
            -0.5, -0.5, 0.0, 0.5, -0.5, 0.0, 0.5, 0.5, 0.0, -0.5, -0.5, 0.0, 0.5, 0.5, 0.0, -0.5,
            0.5, 0.0,
        ],
    );
    let vao = vb.build().expect("vao");
    let material_layout = ctx
        .create_param_layout(&ParamLayoutBuilder::new().uniform("u_color", UniformType::Vec4))
        .unwrap();
    let material = |rgba: [f32; 4]| -> ParamGroupId {
        let g = ctx.create_param_group(material_layout).unwrap();
        ctx.param_set_floats_by_name(g, "u_color", &rgba);
        g
    };
    let mesh = |material| MeshRenderer {
        program,
        state,
        vao,
        material,
        command: DrawCommand::arrays(DrawMode::Triangles, 0, 6),
        transparent: false,
        transmissive: false,
    };
    let red = mesh(material([1.0, 0.0, 0.0, 1.0]));
    let green = mesh(material([0.0, 1.0, 0.0, 1.0]));

    // Orthographic, 4 units across, looking down −Z from z = 5: x = −1 is a
    // quarter of the way in, x = +1 three quarters.
    let mut world = World::new();
    let camera = world.spawn();
    world.insert(camera, Camera::orthographic(2.0, 1.0, 0.1, 100.0));
    world.insert(
        camera,
        Transform::from_translation(Vec3::new(0.0, 0.0, 5.0)),
    );

    let mut pass = Pass::to_default();
    pass.set_clear(&ClearMask::color());
    let scene = SceneRenderer::new(&ctx).expect("scene");
    Setup {
        ctx,
        gl,
        world,
        transforms: TransformSystem::new(),
        scene,
        pass,
        camera,
        red,
        green,
    }
}

impl Setup {
    fn object(&mut self, mesh: MeshRenderer, x: f32) -> Entity {
        let e = self.world.spawn();
        self.world
            .insert(e, Transform::from_translation(Vec3::new(x, 0.0, 0.0)));
        self.world
            .insert(e, Bounds::new(Vec3::splat(-0.5), Vec3::splat(0.5)));
        self.world.insert(e, mesh);
        e
    }

    fn frame(&mut self) -> RenderStats {
        self.transforms.update(&mut self.world);
        self.pass.clear_draws();
        let stats = self
            .scene
            .render(&self.world, &self.ctx, self.camera, &mut self.pass);
        self.pass.sort();
        self.ctx.render(&self.pass);
        self.world.clear_trackers();
        stats
    }

    // Pixels at x = −1, 0, +1 (world) on the y = 0 line.
    fn read(&self) -> [[u8; 4]; 3] {
        let mut out = [[0u8; 4]; 3];
        for (i, px) in out.iter_mut().enumerate() {
            let x = SIZE / 4 + i as i32 * SIZE / 4;
            self.gl
                .read_pixels_with_opt_u8_array(
                    x,
                    SIZE / 2,
                    1,
                    1,
                    Gl::RGBA,
                    Gl::UNSIGNED_BYTE,
                    Some(&mut px[..]),
                )
                .unwrap();
        }
        assert_eq!(self.gl.get_error(), Gl::NO_ERROR);
        out
    }
}

fn stats(drawn: usize, culled: usize, objects_written: usize) -> RenderStats {
    // Own shaders (object uniforms): a draw per mesh.
    RenderStats {
        drawn,
        draws: drawn,
        culled,
        objects_written,
        pending: 0,
        transmissive: 0,
        instanced: 0,
    }
}

#[wasm_bindgen_test]
fn draws_through_the_camera_and_rewrites_only_moved_objects() {
    let mut s = setup();
    let (red, green) = (s.red, s.green);
    let a = s.object(red, -1.0);
    let b = s.object(green, 1.0);
    assert_eq!(s.frame(), stats(2, 0, 2));
    assert_eq!(s.read(), [RED, BLACK, GREEN]);

    assert_eq!(s.frame(), stats(2, 0, 0), "still frame: no object writes");
    assert_eq!(s.read(), [RED, BLACK, GREEN]);

    s.world.get_mut::<Transform>(b).unwrap().translation.x = 0.0;
    assert_eq!(s.frame(), stats(2, 0, 1));
    assert_eq!(s.read(), [RED, GREEN, BLACK]);

    // Moving the camera changes the frame block, not the objects.
    s.world
        .get_mut::<Transform>(s.camera)
        .unwrap()
        .translation
        .x = -1.0;
    assert_eq!(s.frame(), stats(2, 0, 0));
    assert_eq!(s.read(), [BLACK, RED, GREEN]);

    // Children follow their parent.
    s.world
        .get_mut::<Transform>(s.camera)
        .unwrap()
        .translation
        .x = 0.0;
    set_parent(&mut s.world, b, Some(a)).unwrap();
    s.world
        .insert(b, Transform::from_translation(Vec3::new(2.0, 0.0, 0.0)));
    assert_eq!(s.frame(), stats(2, 0, 2), "rebuild rewrites everything");
    assert_eq!(s.read(), [RED, BLACK, GREEN]);
}

#[wasm_bindgen_test]
fn culled_objects_catch_up_when_they_come_back() {
    let mut s = setup();
    let (red, green) = (s.red, s.green);
    s.object(red, -1.0);
    let b = s.object(green, 1.0);
    s.frame();

    s.world.get_mut::<Transform>(b).unwrap().translation.x = 10.0;
    assert_eq!(
        s.frame(),
        stats(1, 1, 0),
        "culled before its group is written"
    );
    assert_eq!(s.read(), [RED, BLACK, BLACK]);

    // The camera follows it there: b didn't move this frame, but its group
    // still holds the matrix from before it left view.
    s.world
        .get_mut::<Transform>(s.camera)
        .unwrap()
        .translation
        .x = 10.0;
    assert_eq!(s.frame(), stats(1, 1, 1));
    assert_eq!(s.read(), [BLACK, GREEN, BLACK]);

    s.world
        .get_mut::<Transform>(s.camera)
        .unwrap()
        .translation
        .x = 0.0;
    s.world.get_mut::<Transform>(b).unwrap().translation.x = 0.0;
    assert_eq!(s.frame(), stats(2, 0, 1));
    assert_eq!(s.read(), [RED, GREEN, BLACK]);

    // Without Bounds an object is never culled (drawn off-screen).
    s.world.remove::<Bounds>(b);
    s.world.get_mut::<Transform>(b).unwrap().translation.x = 10.0;
    assert_eq!(s.frame(), stats(2, 0, 1));
}

#[wasm_bindgen_test]
fn hidden_removed_and_despawned_objects_are_not_drawn() {
    let mut s = setup();
    let (red, green) = (s.red, s.green);
    let a = s.object(red, -1.0);
    let b = s.object(green, 1.0);
    s.frame();
    assert!(s.scene.object_group(a).is_some());

    s.world.insert(b, Visible(false));
    assert_eq!(s.frame(), stats(1, 0, 0));
    assert_eq!(s.read(), [RED, BLACK, BLACK]);
    s.world.insert(b, Visible(true));
    assert_eq!(s.frame().drawn, 2);

    s.world.remove::<MeshRenderer>(b);
    s.world.despawn(a);
    assert_eq!(s.frame(), stats(0, 0, 0));
    assert_eq!(s.read(), [BLACK, BLACK, BLACK]);
    assert!(s.scene.object_group(a).is_none(), "group released");
    assert!(s.scene.object_group(b).is_none(), "group released");
}

#[wasm_bindgen_test]
fn a_camera_without_a_camera_component_draws_nothing() {
    let mut s = setup();
    let red = s.red;
    s.object(red, 0.0);
    s.world.remove::<Camera>(s.camera);
    assert_eq!(s.frame(), stats(0, 0, 0));
    assert_eq!(s.read(), [BLACK, BLACK, BLACK]);
}
