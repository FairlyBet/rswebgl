//! Motion vectors for temporal antialiasing, for what moved since the last
//! frame only (as Unreal does): the camera's own motion is reprojected
//! through the depth (taa.frag), exact for everything static. A mesh whose
//! model matrix, joints or morph weights changed is drawn once more, after
//! the scene, into a velocity target with its last-frame state next to the
//! current one in the instance texture (`motion.vert`). A static scene
//! costs nothing; custom shaders need nothing.

use std::collections::HashMap;

use glam::{Mat4, Vec2};
use rswebgl::console;
use rswebgl::context::Context;
use rswebgl::draw::DrawCommand;
use rswebgl::error::Error;
use rswebgl::ids::{ParamGroupId, ParamLayoutId, ProgramId, StateId, TextureId, VertexArrayId};
use rswebgl::param::{GROUP_SLOTS, ParamField, ParamLayoutBuilder};
use rswebgl::pass::Pass;
use rswebgl::render_state::RenderState;
use rswebgl::uniform_buffer::UniformType;
use rswebgl_ecs::{Entity, World};
use rswebgl_gltf::{ATTR_MORPH_NORMAL, ATTR_MORPH_POSITION, ATTR_MORPH_TANGENT};

use crate::animation::MorphWeights;
use crate::instances::Instances;
use crate::model::{Morph, Primitive, Skin};
use crate::render::{FRAME_SLOT, MATERIAL_SLOT, MeshRenderer, OBJECT_SLOT};
use crate::shader::create_scene_program_with;
use crate::transform::{Parent, WorldMatrix};

const VERT: &str = include_str!("../shaders/motion.vert");
const FRAG: &str = include_str!("../shaders/motion.frag");

// What a mesh was drawn with: compared frame to frame.
#[derive(Default, PartialEq)]
struct State {
    model: Mat4,
    joints: Vec<Mat4>,
    weights: Vec<f32>,
}

// A moving mesh's motion draw: where its state now and last frame start in
// the instance texture.
struct MotionDraw {
    variant: u32,
    vao: VertexArrayId,
    command: DrawCommand,
    model: i32,
    prev_model: i32,
    joints: i32,
    prev_joints: i32,
    morph: i32,
    prev_morph: i32,
    morph_count: i32,
    morph_texture: TextureId,
}

// Variant bits: skinned, and the morph kinds (when positions morph).
const VARIANT_SKIN: u32 = 1;
const VARIANT_KINDS_SHIFT: u32 = 1;

pub(crate) struct MotionVectors {
    // Each drawn mesh's state last frame, and this frame's so far.
    prev: HashMap<Entity, State>,
    next: HashMap<Entity, State>,
    prev_view_proj: Mat4,
    draws: Vec<MotionDraw>,
    // Whether a mesh drawn without motion vectors (transparent,
    // transmissive) moved this frame: the resolve's reactive mask then
    // applies where the camera holds still too.
    late_moved: bool,
    // Built on first need; None once it failed.
    programs: HashMap<u32, Option<ProgramId>>,
    globals_layout: ParamLayoutId,
    globals: ParamGroupId,
    f_prev_view_proj: ParamField,
    f_motion: ParamField,
    f_scene_depth: ParamField,
    object_layout: ParamLayoutId,
    groups: Vec<ParamGroupId>,
    f: [ParamField; 7],
    f_morph_targets: ParamField,
    state: StateId,
}

const OBJECT_FIELDS: [&str; 7] = [
    "u_instance_base",
    "u_prev_base",
    "u_joint_base",
    "u_prev_joint_base",
    "u_morph_base",
    "u_prev_morph_base",
    "u_morph_count",
];

impl MotionVectors {
    pub fn new(ctx: &Context) -> Result<Self, Error> {
        let globals_layout = ctx.create_param_layout(
            &ParamLayoutBuilder::new()
                .uniform("u_prev_view_proj", UniformType::Mat4)
                .uniform("u_motion", UniformType::Vec4)
                .texture("u_scene_depth"),
        )?;
        let mut object = ParamLayoutBuilder::new();
        for name in OBJECT_FIELDS {
            object = object.uniform(name, UniformType::I32);
        }
        let object_layout = ctx.create_param_layout(&object.texture("u_morph_targets"))?;
        let mut f = [ParamField::default(); 7];
        for (slot, name) in f.iter_mut().zip(OBJECT_FIELDS) {
            *slot = ctx.param_field(object_layout, name)?;
        }
        let mut state = RenderState::new();
        state.depth_test = false;
        state.depth_mask = false;
        state.cull_face = false;
        let mut motion = Self {
            prev: HashMap::new(),
            next: HashMap::new(),
            prev_view_proj: Mat4::IDENTITY,
            draws: Vec::new(),
            late_moved: false,
            programs: HashMap::new(),
            globals: ctx.create_param_group(globals_layout)?,
            f_prev_view_proj: ctx.param_field(globals_layout, "u_prev_view_proj")?,
            f_motion: ctx.param_field(globals_layout, "u_motion")?,
            f_scene_depth: ctx.param_field(globals_layout, "u_scene_depth")?,
            globals_layout,
            object_layout,
            groups: Vec::new(),
            f,
            f_morph_targets: ctx.param_field(object_layout, "u_morph_targets")?,
            state: ctx.render_state(&state),
        };
        // The rigid variant, compiling in the background from now on.
        motion.program(ctx, 0);
        Ok(motion)
    }

    /// The motion shaders built so far (to wait for).
    pub fn programs(&self) -> impl Iterator<Item = ProgramId> + '_ {
        self.programs.values().flatten().copied()
    }

    /// A new frame: what was drawn becomes last frame's; `prev_view_proj`
    /// is last frame's camera (unjittered).
    pub fn begin(&mut self, prev_view_proj: Mat4) {
        std::mem::swap(&mut self.prev, &mut self.next);
        self.next.clear();
        self.draws.clear();
        self.late_moved = false;
        self.prev_view_proj = prev_view_proj;
    }

    /// Notes a mesh drawn this frame (with model matrix `m`). If it moved
    /// since the last: an `opaque` one gets its state then and now in
    /// `instances` and a motion draw; another (transparent, transmissive:
    /// the velocity target holds what's behind it) counts for
    /// `late_moved`.
    pub fn track(
        &mut self,
        world: &World,
        e: Entity,
        m: &Mat4,
        mesh: &MeshRenderer,
        opaque: bool,
        instances: &mut Instances,
    ) {
        if mesh.command.instance_count != 1 {
            return; // draws its own instances: their motion is unknown
        }
        let mut now = State {
            model: *m,
            ..State::default()
        };
        let skin = world.get::<Skin>(e);
        if let Some(skin) = skin {
            now.joints
                .extend(skin.joints.iter().enumerate().map(|(j, &joint)| {
                    let w = world
                        .get::<WorldMatrix>(joint)
                        .map_or(Mat4::IDENTITY, |w| w.0);
                    w * skin.inverse_bind.get(j).copied().unwrap_or(Mat4::IDENTITY)
                }));
        }
        // Morph targets matter here only when they move positions.
        let morph = world.get::<Morph>(e).and_then(|morph| {
            let attributes = world.get::<Primitive>(e)?.attributes;
            if attributes & ATTR_MORPH_POSITION == 0 {
                return None;
            }
            let kinds = (attributes
                & (ATTR_MORPH_POSITION | ATTR_MORPH_NORMAL | ATTR_MORPH_TANGENT))
                .count_ones();
            Some((*morph, kinds))
        });
        if let Some((morph, _)) = morph {
            let node = world.get::<Parent>(e).map(|p| p.0);
            let weights = node.and_then(|n| world.get::<MorphWeights>(n));
            now.weights.extend(weights.map_or(&[][..], |w| &w.0[..]));
            now.weights.resize(morph.targets as usize, 0.0);
        }
        if !opaque {
            if self.prev.get(&e).is_some_and(|prev| *prev != now) {
                self.late_moved = true;
            }
            self.next.insert(e, now);
            return;
        }
        if let Some(prev) = self.prev.get(&e)
            && *prev != now
            && prev.joints.len() == now.joints.len()
            && prev.weights.len() == now.weights.len()
        {
            let model = instances.push(&now.model);
            let prev_model = instances.push(&prev.model);
            let (mut joints, mut prev_joints) = (0, 0);
            if skin.is_some() {
                joints = instances.len();
                for j in &now.joints {
                    instances.push(j);
                }
                prev_joints = instances.len();
                for j in &prev.joints {
                    instances.push(j);
                }
            }
            let mut variant = if skin.is_some() { VARIANT_SKIN } else { 0 };
            let (mut morph_base, mut prev_morph, mut morph_count) = (0, 0, 0);
            let mut morph_texture = TextureId::NULL;
            if let Some((morph, kinds)) = morph {
                morph_base = instances.push_floats(&now.weights, now.weights.len());
                prev_morph = instances.push_floats(&prev.weights, prev.weights.len());
                morph_count = morph.targets as i32;
                morph_texture = morph.texture;
                variant |= kinds << VARIANT_KINDS_SHIFT;
            }
            self.draws.push(MotionDraw {
                variant,
                vao: mesh.vao,
                command: mesh.command,
                model,
                prev_model,
                joints,
                prev_joints,
                morph: morph_base,
                prev_morph,
                morph_count,
                morph_texture,
            });
        }
        self.next.insert(e, now);
    }

    /// Meshes that moved this frame.
    pub fn count(&self) -> usize {
        self.draws.len()
    }

    /// Whether a transparent or transmissive mesh moved this frame.
    pub fn late_moved(&self) -> bool {
        self.late_moved
    }

    /// Whether anything moved this frame.
    pub fn any(&self) -> bool {
        !self.draws.is_empty()
    }

    /// This frame's motion draws into `pass` (the velocity target, after the
    /// scene's passes and the instance upload), tested against
    /// `scene_depth`. `frame` is the scene's frame group; `jitter` the
    /// projection's offset this frame. Returns the draws added (a mesh
    /// whose motion shader is still compiling waits).
    pub fn push_draws(
        &mut self,
        ctx: &Context,
        pass: &mut Pass,
        frame: ParamGroupId,
        scene_depth: TextureId,
        jitter: Vec2,
    ) -> usize {
        ctx.param_set_floats(
            self.globals,
            self.f_prev_view_proj,
            &self.prev_view_proj.to_cols_array(),
        );
        let reverse = if ctx.reverse_z() { 1.0 } else { 0.0 };
        ctx.param_set_floats(
            self.globals,
            self.f_motion,
            &[jitter.x, jitter.y, reverse, 0.0],
        );
        ctx.param_set_texture(self.globals, self.f_scene_depth, scene_depth);
        let mut added = 0;
        let draws = std::mem::take(&mut self.draws);
        for d in &draws {
            let Some(program) = self.program(ctx, d.variant) else {
                continue;
            };
            if ctx.program_link_pending(program) {
                continue;
            }
            let Some(group) = self.group(ctx, added) else {
                break;
            };
            let values = [
                d.model,
                d.prev_model,
                d.joints,
                d.prev_joints,
                d.morph,
                d.prev_morph,
                d.morph_count,
            ];
            for (f, v) in self.f.iter().zip(values) {
                ctx.param_set_i32(group, *f, v);
            }
            if !d.morph_texture.is_null() {
                ctx.param_set_texture(group, self.f_morph_targets, d.morph_texture);
            }
            let mut groups = [ParamGroupId::NULL; GROUP_SLOTS];
            groups[FRAME_SLOT] = frame;
            groups[MATERIAL_SLOT] = self.globals;
            groups[OBJECT_SLOT] = group;
            pass.push_groups(program, self.state, d.vao, groups, &d.command);
            added += 1;
        }
        self.draws = draws;
        added
    }

    fn program(&mut self, ctx: &Context, variant: u32) -> Option<ProgramId> {
        *self.programs.entry(variant).or_insert_with(|| {
            let mut defines = String::new();
            if variant & VARIANT_SKIN != 0 {
                defines.push_str("#define HAS_SKIN\n");
            }
            let kinds = variant >> VARIANT_KINDS_SHIFT;
            if kinds > 0 {
                defines.push_str(&format!("#define MORPH_KINDS {kinds}\n"));
            }
            match create_scene_program_with(ctx, VERT, FRAG, &defines) {
                Ok(p) => Some(p),
                Err(err) => {
                    console::error_once(&format!("[rswebgl-scene] motion vectors: {err}"));
                    None
                }
            }
        })
    }

    fn group(&mut self, ctx: &Context, i: usize) -> Option<ParamGroupId> {
        while self.groups.len() <= i {
            match ctx.create_param_group(self.object_layout) {
                Ok(g) => self.groups.push(g),
                Err(err) => {
                    console::error_once(&format!("[rswebgl-scene] motion vectors: {err}"));
                    return None;
                }
            }
        }
        Some(self.groups[i])
    }

    /// Frees it (programs only while the context they belong to lives).
    pub fn destroy(self, ctx: &Context, lost: bool) {
        for g in self.groups {
            ctx.destroy_param_group(g);
        }
        ctx.destroy_param_group(self.globals);
        ctx.destroy_param_layout(self.globals_layout);
        ctx.destroy_param_layout(self.object_layout);
        if !lost {
            for p in self.programs.into_values().flatten() {
                ctx.destroy_program(p);
            }
        }
    }
}
