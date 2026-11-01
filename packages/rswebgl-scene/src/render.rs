//! Scene → draw list: `SceneRenderer` turns entities with a `MeshRenderer`
//! into draws in a `Pass`, with the camera's matrices in a per-frame uniform
//! block and each object's matrices in its own parameter group.

use std::collections::HashMap;

use glam::{Mat3, Mat4};
use rswebgl::buffer::BufferUsage;
use rswebgl::console;
use rswebgl::context::Context;
use rswebgl::draw::DrawCommand;
use rswebgl::error::Error;
use rswebgl::ids::{
    ParamGroupId, ParamLayoutId, ProgramId, StateId, UniformBufferId, VertexArrayId,
};
use rswebgl::param::{GROUP_SLOTS, ParamField, ParamLayoutBuilder};
use rswebgl::pass::Pass;
use rswebgl::uniform_buffer::{UboLayout, UniformType};
use rswebgl_ecs::{Entity, Tick, World};

use crate::camera::Camera;
use crate::culling::{Bounds, Frustum};
use crate::transform::WorldMatrix;

/// Name of the per-frame uniform block. Shaders declare it as
///
/// ```glsl
/// layout(std140) uniform Frame {
///     mat4 u_view;
///     mat4 u_proj;
///     mat4 u_view_proj;
///     vec4 u_camera_pos; // xyz; w = 1
/// };
/// ```
///
/// The block binds by name; declare all four members, in this order.
pub const FRAME_BLOCK: &str = "Frame";
/// Per-object uniforms, set in group slot 2: `uniform mat4 u_model;` and
/// `uniform mat3 u_normal_matrix;` (inverse-transpose of the model's upper
/// 3×3). A shader may declare either or neither.
pub const MODEL_UNIFORM: &str = "u_model";
pub const NORMAL_MATRIX_UNIFORM: &str = "u_normal_matrix";

/// Group slots the scene fills: 0 frame, 1 material, 2 object.
pub const FRAME_SLOT: usize = 0;
pub const MATERIAL_SLOT: usize = 1;
pub const OBJECT_SLOT: usize = 2;

/// Draws the entity's mesh. Everything is a renderer handle: the scene neither
/// creates nor destroys them.
#[derive(Debug, Clone, Copy)]
pub struct MeshRenderer {
    pub program: ProgramId,
    pub state: StateId,
    pub vao: VertexArrayId,
    /// Slot-1 group (NULL = none). Draws are sorted by it after program and
    /// state, so objects sharing a material draw together.
    pub material: ParamGroupId,
    pub command: DrawCommand,
}

/// Hides the entity when false. No component = visible. Not inherited:
/// hiding a parent leaves its children drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Visible(pub bool);

/// What the last `SceneRenderer::render` did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RenderStats {
    /// Draws pushed into the pass.
    pub drawn: usize,
    /// Left out by frustum culling.
    pub culled: usize,
    /// Object groups whose matrices were written (moved or new objects).
    pub objects_written: usize,
}

// An entity's object group and the `WorldMatrix` change tick it holds. Kept
// per object rather than per run: a culled object isn't written, so it has
// to catch up when it comes back into view.
#[derive(Clone, Copy)]
struct Object {
    group: ParamGroupId,
    written: Tick,
}

/// Builds a pass's draw list from the world.
///
/// Per frame, after `TransformSystem::update` and before
/// `World::clear_trackers`:
///
/// ```ignore
/// transforms.update(&mut world);
/// pass.clear_draws();
/// scene.render(&world, &ctx, camera, &mut pass);
/// pass.sort();
/// ctx.render(&pass);
/// world.clear_trackers();
/// ```
///
/// Owns one uniform buffer for the frame block and a parameter group per
/// drawn entity (its matrices, rewritten only when its `WorldMatrix`
/// changes). Groups of entities that lost their `MeshRenderer` or were
/// despawned are destroyed on the next `render`. Release everything with
/// `destroy`; after a context loss, build a new `SceneRenderer`.
pub struct SceneRenderer {
    frame_ubo: UniformBufferId,
    frame_layout: ParamLayoutId,
    frame_group: ParamGroupId,
    object_layout: ParamLayoutId,
    f_model: ParamField,
    f_normal: ParamField,
    objects: HashMap<Entity, Object>,
    stats: RenderStats,
}

impl SceneRenderer {
    pub fn new(ctx: &Context) -> Result<Self, Error> {
        let frame_ubo = ctx.create_uniform_buffer(
            &UboLayout::new()
                .field("u_view", UniformType::Mat4)
                .field("u_proj", UniformType::Mat4)
                .field("u_view_proj", UniformType::Mat4)
                .field("u_camera_pos", UniformType::Vec4),
            BufferUsage::DynamicDraw,
        )?;
        let frame_layout =
            ctx.create_param_layout(&ParamLayoutBuilder::new().block(FRAME_BLOCK))?;
        let frame_group = ctx.create_param_group(frame_layout)?;
        ctx.param_set_block_by_name(frame_group, FRAME_BLOCK, frame_ubo);
        let object_layout = ctx.create_param_layout(
            &ParamLayoutBuilder::new()
                .uniform(MODEL_UNIFORM, UniformType::Mat4)
                .uniform(NORMAL_MATRIX_UNIFORM, UniformType::Mat3),
        )?;
        Ok(Self {
            frame_ubo,
            frame_layout,
            frame_group,
            object_layout,
            f_model: ctx.param_field(object_layout, MODEL_UNIFORM)?,
            f_normal: ctx.param_field(object_layout, NORMAL_MATRIX_UNIFORM)?,
            objects: HashMap::new(),
            stats: RenderStats::default(),
        })
    }

    /// The uniform buffer behind the frame block — e.g. to bind it into other
    /// passes' groups.
    pub fn frame_ubo(&self) -> UniformBufferId {
        self.frame_ubo
    }

    /// The slot-0 group every scene draw uses.
    pub fn frame_group(&self) -> ParamGroupId {
        self.frame_group
    }

    /// The object group of `e`, once it has been drawn.
    pub fn object_group(&self, e: Entity) -> Option<ParamGroupId> {
        self.objects.get(&e).map(|o| o.group)
    }

    pub fn stats(&self) -> RenderStats {
        self.stats
    }

    /// Appends the visible `MeshRenderer`s, as seen from `camera`, to `pass`.
    /// Doesn't clear or sort the pass. An entity without a `WorldMatrix` yet
    /// (run `TransformSystem::update` first) is skipped.
    pub fn render(
        &mut self,
        world: &World,
        ctx: &Context,
        camera: Entity,
        pass: &mut Pass,
    ) -> RenderStats {
        self.stats = RenderStats::default();
        self.release_removed(world, ctx);

        let Some(view_proj) = self.write_frame(world, ctx, camera) else {
            return self.stats;
        };
        let frustum = Frustum::from_view_proj(&view_proj);

        let (Some(meshes), Some(worlds)) = (
            world.storage::<MeshRenderer>(),
            world.storage::<WorldMatrix>(),
        ) else {
            return self.stats;
        };
        let bounds = world.storage::<Bounds>();
        let visible = world.storage::<Visible>();
        for (e, mesh) in meshes.iter() {
            if visible.and_then(|v| v.get(e)).is_some_and(|v| !v.0) {
                continue;
            }
            let Some(row) = worlds.row(e) else {
                continue;
            };
            let m = worlds.data()[row as usize].0;
            if let Some(b) = bounds.and_then(|b| b.get(e)) {
                let (c, h) = b.transformed(&m);
                if !frustum.intersects(c, h) {
                    self.stats.culled += 1;
                    continue;
                }
            }

            let tick = worlds.changed_tick(row);
            let group = match self.objects.get(&e).copied() {
                Some(o) => {
                    if o.written != tick {
                        self.write_object(ctx, o.group, &m);
                        self.objects.insert(e, Object { written: tick, ..o });
                    }
                    o.group
                }
                None => match ctx.create_param_group(self.object_layout) {
                    Ok(group) => {
                        self.write_object(ctx, group, &m);
                        self.objects.insert(
                            e,
                            Object {
                                group,
                                written: tick,
                            },
                        );
                        group
                    }
                    Err(err) => {
                        console::error_once(&format!(
                            "[rswebgl-scene] can't create an object group: {err}"
                        ));
                        continue;
                    }
                },
            };

            let mut groups = [ParamGroupId::NULL; GROUP_SLOTS];
            groups[FRAME_SLOT] = self.frame_group;
            groups[MATERIAL_SLOT] = mesh.material;
            groups[OBJECT_SLOT] = group;
            pass.push_groups(mesh.program, mesh.state, mesh.vao, groups, &mesh.command);
            self.stats.drawn += 1;
        }
        self.stats
    }

    /// Destroys every resource the scene renderer created. Draws already in a
    /// pass then refer to stale groups.
    pub fn destroy(self, ctx: &Context) {
        for o in self.objects.into_values() {
            ctx.destroy_param_group(o.group);
        }
        ctx.destroy_param_group(self.frame_group);
        ctx.destroy_param_layout(self.frame_layout);
        ctx.destroy_param_layout(self.object_layout);
        ctx.destroy_uniform_buffer(self.frame_ubo);
    }

    // Camera matrices into the frame block; the view-projection for culling.
    fn write_frame(&self, world: &World, ctx: &Context, camera: Entity) -> Option<Mat4> {
        let (Some(cam), Some(cam_world)) = (
            world.get::<Camera>(camera),
            world.get::<WorldMatrix>(camera),
        ) else {
            console::error_once(
                "[rswebgl-scene] the camera entity needs a Camera and a WorldMatrix \
                 (a Transform, after TransformSystem::update); nothing drawn",
            );
            return None;
        };
        let view = cam_world.0.inverse();
        let proj = cam.projection_matrix();
        let view_proj = proj * view;
        let eye = cam_world.0.w_axis;
        ctx.ubo_set_mat(self.frame_ubo, "u_view", &view.to_cols_array());
        ctx.ubo_set_mat(self.frame_ubo, "u_proj", &proj.to_cols_array());
        ctx.ubo_set_mat(self.frame_ubo, "u_view_proj", &view_proj.to_cols_array());
        ctx.ubo_set_vec4(self.frame_ubo, "u_camera_pos", eye.x, eye.y, eye.z, 1.0);
        Some(view_proj)
    }

    fn write_object(&mut self, ctx: &Context, group: ParamGroupId, m: &Mat4) {
        let upper = Mat3::from_mat4(*m);
        // A zero scale has no inverse; fall back rather than upload NaNs.
        let normal = if upper.determinant().abs() > f32::EPSILON {
            upper.inverse().transpose()
        } else {
            upper
        };
        ctx.param_set_floats(group, self.f_model, &m.to_cols_array());
        ctx.param_set_floats(group, self.f_normal, &normal.to_cols_array());
        self.stats.objects_written += 1;
    }

    // Groups of entities that no longer have a MeshRenderer. Seen through the
    // removal log; a sweep also runs if there are more groups than meshes
    // (removals missed because `clear_trackers` ran first).
    fn release_removed(&mut self, world: &World, ctx: &Context) {
        let meshes = world.storage::<MeshRenderer>();
        let logged = meshes.is_some_and(|m| !m.removed().is_empty());
        let live = meshes.map_or(0, |m| m.len());
        if !logged && self.objects.len() <= live {
            return;
        }
        self.objects.retain(|&e, o| {
            let keep = world.has::<MeshRenderer>(e);
            if !keep {
                ctx.destroy_param_group(o.group);
            }
            keep
        });
    }
}
