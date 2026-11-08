// A grid of cubes built as a scene: entities with Transform / Parent /
// MeshRenderer / Bounds, turned into a draw list by SceneRenderer. The grid
// hangs off one pivot entity that turns slowly, so the whole grid moves
// through the hierarchy; every other cube also spins on its own. Cubes that
// leave the view are culled — the corner shows drawn / culled / rewritten.

use std::cell::RefCell;
use std::rc::Rc;

use glam::{Mat4, Quat, Vec3};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;

use rswebgl::context::{Context, ContextOptions};
use rswebgl::draw::{DrawCommand, DrawMode, IndexType};
use rswebgl::framebuffer::ClearMask;
use rswebgl::param::ParamLayoutBuilder;
use rswebgl::pass::Pass;
use rswebgl::render_state::{CullFace, RenderState};
use rswebgl::uniform_buffer::UniformType;
use rswebgl::vao_builder::{AttrKind, VaoBuilder};
use rswebgl_ecs::{Entity, World};
use rswebgl_scene::{
    Bounds, Camera, MeshRenderer, SceneRenderer, Transform, TransformSystem, set_parent,
};

// Matrices come from the scene: the frame block (camera) and u_model (object).
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
out vec3 v_world;
void main() {
    vec4 world = u_model * vec4(a_pos, 1.0);
    v_world = world.xyz;
    gl_Position = u_view_proj * world;
}
"#;

// Flat shading from screen-space derivatives: the cube needs no normals.
const FRAG: &str = r#"#version 300 es
precision highp float;
in vec3 v_world;
uniform vec3 u_color;
out vec4 frag_color;
void main() {
    vec3 n = normalize(cross(dFdx(v_world), dFdy(v_world)));
    float light = 0.35 + 0.65 * max(dot(n, normalize(vec3(0.4, 0.8, 0.5))), 0.0);
    frag_color = vec4(u_color * light, 1.0);
}
"#;

const GRID: i32 = 24; // GRID × GRID cubes
const SPACING: f32 = 1.6;

type FrameFn = Closure<dyn FnMut(f64)>;

struct App {
    ctx: Context,
    world: World,
    transforms: TransformSystem,
    scene: SceneRenderer,
    pass: Pass,
    camera: Entity,
    pivot: Entity,
    spinners: Vec<Entity>,
}

fn build(ctx: &Context) -> Result<App, String> {
    let program = ctx.create_program(VERT, FRAG)?;
    let mut state = RenderState::new();
    state.depth_test = true;
    state.cull_face = true;
    state.cull_mode = CullFace::Back;
    let state = ctx.render_state(&state);

    #[rustfmt::skip]
    let positions = [
        -0.5, -0.5, -0.5,  0.5, -0.5, -0.5,  0.5, 0.5, -0.5,  -0.5, 0.5, -0.5,
        -0.5, -0.5,  0.5,  0.5, -0.5,  0.5,  0.5, 0.5,  0.5,  -0.5, 0.5,  0.5,
    ];
    #[rustfmt::skip]
    let indices = [
        0, 2, 1, 0, 3, 2,  4, 5, 6, 4, 6, 7,  0, 4, 7, 0, 7, 3,
        1, 2, 6, 1, 6, 5,  0, 1, 5, 0, 5, 4,  3, 7, 6, 3, 6, 2,
    ];
    let mut vb = VaoBuilder::new(ctx);
    vb.add_f32(0, AttrKind::F32x3, &positions);
    vb.add_indices_u16(&indices);
    let vao = vb.build()?;
    let bounds = Bounds::from_points(&positions).expect("cube has points");

    let material_layout =
        ctx.create_param_layout(&ParamLayoutBuilder::new().uniform("u_color", UniformType::Vec3))?;
    let material = |rgb: [f32; 3]| -> Result<_, String> {
        let g = ctx.create_param_group(material_layout)?;
        ctx.param_set_floats_by_name(g, "u_color", &rgb);
        Ok(g)
    };
    let materials = [material([0.9, 0.55, 0.2])?, material([0.3, 0.6, 0.9])?];
    let command = DrawCommand::elements(DrawMode::Triangles, 36, IndexType::UnsignedShort, 0);

    let mut world = World::new();
    let pivot = world.spawn();
    world.insert(pivot, Transform::IDENTITY);

    let mut spinners = Vec::new();
    for i in 0..GRID * GRID {
        let (gx, gz) = (i % GRID, i / GRID);
        let half = (GRID - 1) as f32 / 2.0;
        let e = world.spawn();
        world.insert(
            e,
            Transform::from_translation(Vec3::new(
                (gx as f32 - half) * SPACING,
                0.0,
                (gz as f32 - half) * SPACING,
            )),
        );
        world.insert(e, bounds);
        world.insert(
            e,
            MeshRenderer {
                program,
                state,
                vao,
                material: materials[(i % 2) as usize],
                command,
                transparent: false,
                transmissive: false,
            },
        );
        set_parent(&mut world, e, Some(pivot)).map_err(|e| e.to_string())?;
        if (gx + gz) % 2 == 0 {
            spinners.push(e);
        }
    }

    let camera = world.spawn();
    let eye = Vec3::new(0.0, 7.0, 16.0);
    world.insert(
        camera,
        Transform::from_matrix(Mat4::look_at_rh(eye, Vec3::ZERO, Vec3::Y).inverse()),
    );
    world.insert(
        camera,
        Camera::perspective(50f32.to_radians(), 1.0, 0.1, 200.0),
    );

    let mut pass = Pass::to_default();
    pass.set_clear(&ClearMask::color_depth());
    Ok(App {
        ctx: ctx.clone(),
        world,
        transforms: TransformSystem::new(),
        scene: SceneRenderer::new(ctx)?,
        pass,
        camera,
        pivot,
        spinners,
    })
}

impl App {
    fn frame(&mut self, t: f32, aspect: f32) -> String {
        // Animation: plain component writes; the systems pick up what changed.
        self.world
            .get_mut::<Transform>(self.pivot)
            .unwrap()
            .rotation = Quat::from_rotation_y(t * 0.15);
        let spin = Quat::from_rotation_y(t) * Quat::from_rotation_x(t * 0.7);
        for &e in &self.spinners {
            self.world.get_mut::<Transform>(e).unwrap().rotation = spin;
        }
        self.world.get_mut::<Camera>(self.camera).unwrap().aspect = aspect;

        self.transforms.update(&mut self.world);
        self.pass.clear_draws();
        let stats = self
            .scene
            .render(&self.world, &self.ctx, self.camera, &mut self.pass);
        self.pass.sort();
        self.ctx.render(&self.pass);
        self.world.clear_trackers();
        format!(
            "drawn {}  culled {}  object groups written {}",
            stats.drawn, stats.culled, stats.objects_written
        )
    }
}

fn main() {
    let window = web_sys::window().expect("no window");
    let document = window.document().expect("no document");
    let canvas: HtmlCanvasElement = document
        .get_element_by_id("c")
        .expect("missing #c canvas")
        .dyn_into()
        .expect("not a canvas");
    let stats_el = document.get_element_by_id("stats").expect("missing #stats");

    let mut options = ContextOptions::new();
    options.antialias = true;
    options.depth = true;
    let ctx = Context::from_canvas(&canvas, &options).expect("ctx");
    let app = Rc::new(RefCell::new(build(&ctx).expect("scene")));

    // After a context loss every handle is stale: build the scene again.
    let restore = Rc::clone(&app);
    ctx.on_restored(move |ctx| match build(ctx) {
        Ok(a) => *restore.borrow_mut() = a,
        Err(e) => web_sys::console::error_1(&format!("rebuild failed: {e}").into()),
    });

    let default_fb = ctx.default_framebuffer();
    default_fb.set_clear_color(0.06, 0.06, 0.08, 1.0);
    default_fb.enable_auto_resize().expect("auto-resize");

    let f: Rc<RefCell<Option<FrameFn>>> = Rc::new(RefCell::new(None));
    let g = f.clone();
    let win = window.clone();
    let mut frames = 0u32;
    *g.borrow_mut() = Some(Closure::<dyn FnMut(f64)>::new(move |time: f64| {
        let aspect = default_fb.width().max(1) as f32 / default_fb.height().max(1) as f32;
        let line = app.borrow_mut().frame(time as f32 / 1000.0, aspect);
        frames += 1;
        if frames.is_multiple_of(15) {
            stats_el.set_text_content(Some(&line));
        }
        let _ = win.request_animation_frame(f.borrow().as_ref().unwrap().as_ref().unchecked_ref());
    }));
    let _ = window.request_animation_frame(g.borrow().as_ref().unwrap().as_ref().unchecked_ref());
}
