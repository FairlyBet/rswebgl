//! Picking on the GPU: which mesh is under a screen point, drawn rather than
//! ray-cast — exact for any geometry the GPU can draw, with no CPU copy of
//! it.
//!
//! Only the one pixel under the point is drawn: the camera's projection is
//! narrowed (a "pick matrix") so that pixel fills a 1×1 target, and the
//! meshes outside that sliver are culled. Two integer attachments receive
//! the entity (`u_object_id`, from its object group) and the depth's bits;
//! both are read back asynchronously, so the answer comes a frame or so
//! later without stalling.
//!
//! Meshes are drawn with their vertex array's location-0 attribute as the
//! position (the loader's convention, and `setMesh`'s for picking); their
//! own shaders aren't used, so a vertex shader that moves vertices (skinning,
//! displacement) is picked where its rest positions are.

use glam::{Mat4, Vec2, Vec3};
use rswebgl::context::Context;
use rswebgl::draw::Viewport;
use rswebgl::error::Error;
use rswebgl::framebuffer::ClearMask;
use rswebgl::ids::{
    FramebufferId, ParamGroupId, ParamLayoutId, ProgramId, RenderbufferId, StateId, TextureId,
};
use rswebgl::param::{GROUP_SLOTS, ParamField, ParamLayoutBuilder};
use rswebgl::pass::Pass;
use rswebgl::readback::ReadFormat;
use rswebgl::render_state::{DepthFunc, RenderState};
use rswebgl::renderbuffer::RenderbufferFormat;
use rswebgl::texture::{TextureFormat, TextureMagFilter, TextureMinFilter, TextureTarget};
use rswebgl::uniform_buffer::UniformType;
use rswebgl_ecs::Entity;

use crate::pick::Hit;
use crate::render::{MeshRenderer, OBJECT_SLOT};

// Standalone, not on the scene prelude: the prelude's std140 blocks count as
// active even unused, and picking binds neither.
const VERT: &str = "#version 300 es\n\
layout(location = 0) in vec3 a_position;\n\
uniform mat4 u_model;\n\
uniform mat4 u_pick_view_proj;\n\
void main() { gl_Position = u_pick_view_proj * u_model * vec4(a_position, 1.0); }\n";

const FRAG: &str = "#version 300 es\n\
precision highp float;\n\
precision highp int;\n\
uniform uint u_object_id;\n\
layout(location = 0) out uint o_entity;\n\
layout(location = 1) out uint o_depth;\n\
void main() { o_entity = u_object_id; o_depth = floatBitsToUint(gl_FragCoord.z); }\n";

// R32UI, filled by rendering (never uploaded).
const R32UI: TextureFormat = TextureFormat {
    internal: 0x8236,  // R32UI
    format: 0x8D94,    // RED_INTEGER
    data_type: 0x1405, // UNSIGNED_INT
};

/// What a pick sees: the narrowed view-projection it draws through, and the
/// camera's own `view_proj` at `ndc` the hit is unprojected with (clip depth
/// in [0, 1] when `zero_to_one`); distances are from `eye`.
#[derive(Clone, Copy)]
pub(crate) struct PickView {
    pub(crate) pick_view_proj: Mat4,
    pub(crate) view_proj: Mat4,
    pub(crate) ndc: Vec2,
    pub(crate) eye: Vec3,
    pub(crate) zero_to_one: bool,
}

/// The GPU objects of picking, made on the first pick.
pub(crate) struct GpuPicker {
    fb: FramebufferId,
    entity_tex: TextureId,
    depth_tex: TextureId,
    depth_rb: RenderbufferId,
    program: ProgramId,
    layout: ParamLayoutId,
    group: ParamGroupId,
    f_view_proj: ParamField,
    state: StateId,
}

impl GpuPicker {
    pub(crate) fn new(ctx: &Context) -> Result<Self, Error> {
        let program = ctx.create_program(VERT, FRAG)?;
        let layout = ctx.create_param_layout(
            &ParamLayoutBuilder::new().uniform("u_pick_view_proj", UniformType::Mat4),
        )?;
        let group = ctx.create_param_group(layout)?;
        let target = || -> Result<TextureId, Error> {
            let t = ctx.create_texture(
                TextureTarget::Texture2D,
                TextureMinFilter::Nearest,
                TextureMagFilter::Nearest,
            )?;
            ctx.texture_storage_2d(t, 1, &R32UI, 1, 1);
            Ok(t)
        };
        let (entity_tex, depth_tex) = (target()?, target()?);
        let depth_rb = ctx.create_renderbuffer(RenderbufferFormat::Depth24, 1, 1)?;
        let fb = ctx.create_framebuffer(1, 1)?;
        ctx.framebuffer_attach_color_texture_2d(fb, 0, entity_tex, 0);
        ctx.framebuffer_attach_color_texture_2d(fb, 1, depth_tex, 0);
        ctx.framebuffer_attach_depth_renderbuffer(fb, depth_rb);
        // Nothing hit: entity 0 (`Entity::NULL`, never alive).
        ctx.framebuffer_set_color_clear_u32(fb, 0, 0, 0, 0, 0);
        ctx.framebuffer_set_color_clear_u32(fb, 1, 0, 0, 0, 0);
        let mut state = RenderState::new();
        state.depth_test = true;
        state.depth_func = DepthFunc::LessEqual;
        Ok(Self {
            fb,
            entity_tex,
            depth_tex,
            depth_rb,
            program,
            layout,
            group,
            f_view_proj: ctx.param_field(layout, "u_pick_view_proj")?,
            state: ctx.render_state(&state),
        })
    }

    /// The pick matrix: maps the pixel at `ndc` (one pixel of a `w`×`h`
    /// target) onto the whole 1×1 pick target.
    pub(crate) fn narrowing(ndc: Vec2, w: f32, h: f32) -> Mat4 {
        let (sx, sy) = (w / 2.0, h / 2.0);
        Mat4::from_cols_array(&[
            sx,
            0.0,
            0.0,
            0.0, //
            0.0,
            sy,
            0.0,
            0.0, //
            0.0,
            0.0,
            1.0,
            0.0, //
            -sx * ndc.x,
            -sy * ndc.y,
            0.0,
            1.0,
        ])
    }

    /// Draws `meshes` (with their object groups) as `view` sees them and
    /// starts reading the pixel back.
    pub(crate) fn pick(
        &self,
        ctx: &Context,
        meshes: &[(MeshRenderer, ParamGroupId)],
        view: &PickView,
    ) -> impl Future<Output = Result<Option<Hit>, Error>> + use<> {
        let PickView {
            pick_view_proj,
            view_proj,
            ndc,
            eye,
            zero_to_one,
        } = *view;
        ctx.param_set_floats(
            self.group,
            self.f_view_proj,
            &pick_view_proj.to_cols_array(),
        );
        let mut pass = Pass::new(self.fb);
        pass.set_viewport(&Viewport::new(0, 0, 1, 1));
        pass.set_clear(&ClearMask::color_depth());
        for (mesh, object) in meshes {
            let mut groups = [ParamGroupId::NULL; GROUP_SLOTS];
            groups[0] = self.group;
            groups[OBJECT_SLOT] = *object;
            pass.push_groups(self.program, self.state, mesh.vao, groups, &mesh.command);
        }
        ctx.render(&pass);
        let entity = ctx.read_pixels_async(self.fb, 0, 0, 0, 1, 1, ReadFormat::RgbaU32);
        let depth = ctx.read_pixels_async(self.fb, 1, 0, 0, 1, 1, ReadFormat::RgbaU32);
        let unproject = view_proj.inverse();
        async move {
            let (entity, depth) = (entity.await?, depth.await?);
            let word = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
            let id = word(&entity);
            if id == 0 {
                return Ok(None);
            }
            // Window depth back to clip-space depth: [0, 1] is the clip range
            // itself under reverse-Z, else it maps from [-1, 1].
            let window = f32::from_bits(word(&depth));
            let z = if zero_to_one {
                window
            } else {
                window * 2.0 - 1.0
            };
            let point = unproject.project_point3(ndc.extend(z));
            Ok(Some(Hit {
                entity: Entity::from_raw(id),
                distance: (point - eye).length(),
                point,
            }))
        }
    }

    pub(crate) fn destroy(self, ctx: &Context) {
        ctx.destroy_framebuffer(self.fb);
        ctx.destroy_texture(self.entity_tex);
        ctx.destroy_texture(self.depth_tex);
        ctx.destroy_renderbuffer(self.depth_rb);
        ctx.destroy_program(self.program);
        ctx.destroy_param_group(self.group);
        ctx.destroy_param_layout(self.layout);
    }
}
