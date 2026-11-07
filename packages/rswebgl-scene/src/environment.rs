//! Image-based lighting: an environment baked into the two cube maps the PBR
//! shader reads — diffuse irradiance and specular radiance prefiltered by
//! roughness (one mip per roughness step) — plus the BRDF lookup table they
//! share.
//!
//! Baking runs on the GPU when the environment is created (a few dozen
//! full-screen draws into RGBA16F cube faces), so it needs half-float render
//! targets — `EXT_color_buffer_half_float` / `EXT_color_buffer_float`, which
//! the context enables when the device has them.

use glam::Vec3;
use rswebgl::context::Context;
use rswebgl::draw::{DrawCommand, DrawMode, Viewport};
use rswebgl::error::Error;
use rswebgl::ids::{FramebufferId, ParamGroupId, ParamLayoutId, ProgramId, TextureId};
use rswebgl::param::{ParamField, ParamLayoutBuilder};
use rswebgl::pass::{Batch, Pass};
use rswebgl::render_state::RenderState;
use rswebgl::texture::{
    CubeMapFace, TextureFormat, TextureMagFilter, TextureMinFilter, TextureTarget, TextureWrap,
};
use rswebgl::uniform_buffer::UniformType;

use crate::brdf_lut::{LUT_SIZE, brdf_lut};
use crate::hdr::HdrImage;

pub(crate) const FULLSCREEN_VERT: &str = include_str!("../shaders/fullscreen.vert");
const BAKE_FRAG: &str = include_str!("../shaders/env_bake.frag");

// RGBA16F storage filled from f32 data (allowed for half-float formats).
const RGBA16F_FROM_F32: TextureFormat = TextureFormat {
    internal: 0x881A,  // RGBA16F
    format: 0x1908,    // RGBA
    data_type: 0x1406, // FLOAT
};

const FACES: [CubeMapFace; 6] = [
    CubeMapFace::PositiveX,
    CubeMapFace::NegativeX,
    CubeMapFace::PositiveY,
    CubeMapFace::NegativeY,
    CubeMapFace::PositiveZ,
    CubeMapFace::NegativeZ,
];

/// Side of the diffuse cube: irradiance is smooth, a few texels per face do.
const DIFFUSE_SIZE: i32 = 32;
/// Roughness steps in the specular cube (mip 0 = mirror, last = rough).
const SPECULAR_MIPS: i32 = 6;

/// An environment's light, ready for the PBR shader: diffuse and specular cube
/// maps. Owns them; `destroy` frees them.
pub struct Environment {
    diffuse: TextureId,
    specular: TextureId,
    specular_mips: u32,
}

impl Environment {
    /// Light of one color from every direction: what `SceneRenderer` uses
    /// until an environment is set (scaled by its intensity, it's a flat
    /// ambient light).
    pub fn constant(ctx: &Context, rgb: Vec3) -> Result<Self, Error> {
        let texel: Vec<u8> = [rgb.x, rgb.y, rgb.z, 1.0]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        let cube = || -> Result<TextureId, Error> {
            let t = cube_texture(ctx, 1, 1, TextureMinFilter::Linear)?;
            for face in FACES {
                ctx.texture_sub_image_cube_face(t, face, 0, 0, 0, 1, 1, &RGBA16F_FROM_F32, &texel);
            }
            Ok(t)
        };
        Ok(Self {
            diffuse: cube()?,
            specular: cube()?,
            specular_mips: 1,
        })
    }

    /// Bakes an equirectangular HDR image (e.g. from `decode_hdr`). `size` is
    /// the specular cube's face side (256 is plenty for lighting; the
    /// background shows it too). Errors if the device can't render to
    /// half-float textures.
    pub fn from_equirect(ctx: &Context, image: &HdrImage, size: u32) -> Result<Self, Error> {
        let size = size.clamp(16, 2048).next_power_of_two() as i32;
        let bake = Baker::new(ctx, size)?;
        let result = bake.run(image, size);
        bake.destroy();
        result
    }

    /// The diffuse irradiance cube (stores E/π: diffuse = albedo × texel).
    pub fn diffuse(&self) -> TextureId {
        self.diffuse
    }

    /// The prefiltered specular cube: mip `i` is roughness `i / (mips − 1)`.
    pub fn specular(&self) -> TextureId {
        self.specular
    }

    pub fn specular_mips(&self) -> u32 {
        self.specular_mips
    }

    pub fn destroy(self, ctx: &Context) {
        ctx.destroy_texture(self.diffuse);
        ctx.destroy_texture(self.specular);
    }
}

/// The split-sum BRDF table (scale, bias by n·v and roughness) as an RGBA16F
/// texture: R scale, G bias.
pub(crate) fn brdf_lut_texture(ctx: &Context) -> Result<TextureId, Error> {
    let tex = ctx.create_texture(
        TextureTarget::Texture2D,
        TextureMinFilter::Linear,
        TextureMagFilter::Linear,
    )?;
    let n = LUT_SIZE as i32;
    ctx.texture_storage_2d(tex, 1, &TextureFormat::rgba16f(), n, n);
    let data: Vec<u8> = brdf_lut()
        .into_iter()
        .flat_map(|[a, b]| [a, b, 0.0, 1.0])
        .flat_map(f32::to_le_bytes)
        .collect();
    ctx.texture_sub_image_2d(tex, 0, 0, 0, n, n, &RGBA16F_FROM_F32, &data);
    Ok(tex)
}

fn cube_texture(
    ctx: &Context,
    size: i32,
    levels: i32,
    min: TextureMinFilter,
) -> Result<TextureId, Error> {
    let t = ctx.create_texture(TextureTarget::TextureCubeMap, min, TextureMagFilter::Linear)?;
    ctx.texture_storage_2d(t, levels, &TextureFormat::rgba16f(), size, size);
    Ok(t)
}

fn mip_count(size: i32) -> i32 {
    32 - (size as u32).leading_zeros() as i32
}

// The GPU side of one bake: programs, a parameter group, a framebuffer.
struct Baker<'a> {
    ctx: &'a Context,
    equirect: ProgramId,
    specular: ProgramId,
    irradiance: ProgramId,
    layout: ParamLayoutId,
    group: ParamGroupId,
    f_face: ParamField,
    f_roughness: ParamField,
    f_source_size: ParamField,
    f_equirect: ParamField,
    f_source: ParamField,
    fb: FramebufferId,
}

impl<'a> Baker<'a> {
    fn new(ctx: &'a Context, size: i32) -> Result<Self, Error> {
        // Linked before use: a bake program that fails to link would make every
        // draw skip and leave the cubes black while the bake "succeeds".
        let program = |mode: &str| -> Result<ProgramId, Error> {
            let head = format!("#version 300 es\n#define {mode}\n");
            let p = ctx.create_program(
                &format!("{head}{FULLSCREEN_VERT}"),
                &format!("{head}{BAKE_FRAG}"),
            )?;
            ctx.program_active_uniform_names(p); // waits for the link
            if !ctx.program_is_valid(p) {
                ctx.destroy_program(p);
                return Err(format!("environment: the {mode} bake shader failed to link").into());
            }
            Ok(p)
        };
        let layout = ctx.create_param_layout(
            &ParamLayoutBuilder::new()
                .uniform("u_face", UniformType::I32)
                .uniform("u_roughness", UniformType::F32)
                .uniform("u_source_size", UniformType::F32)
                .texture("u_equirect")
                .texture("u_source"),
        )?;
        Ok(Self {
            ctx,
            equirect: program("EQUIRECT")?,
            specular: program("SPECULAR")?,
            irradiance: program("IRRADIANCE")?,
            layout,
            group: ctx.create_param_group(layout)?,
            f_face: ctx.param_field(layout, "u_face")?,
            f_roughness: ctx.param_field(layout, "u_roughness")?,
            f_source_size: ctx.param_field(layout, "u_source_size")?,
            f_equirect: ctx.param_field(layout, "u_equirect")?,
            f_source: ctx.param_field(layout, "u_source")?,
            fb: ctx.create_framebuffer(size, size)?,
        })
    }

    fn run(&self, image: &HdrImage, size: i32) -> Result<Environment, Error> {
        let ctx = self.ctx;
        let (w, h) = (image.width as i32, image.height as i32);
        if image.rgb.len() != (w * h * 3) as usize {
            return Err("environment: image data doesn't match its size".into());
        }

        // The equirect image, mipmapped so the cube faces don't alias it.
        let equirect = ctx.create_texture(
            TextureTarget::Texture2D,
            TextureMinFilter::LinearMipmapLinear,
            TextureMagFilter::Linear,
        )?;
        ctx.texture_set_wrap_s(equirect, TextureWrap::Repeat);
        ctx.texture_storage_2d(
            equirect,
            mip_count(w.max(h)),
            &TextureFormat::rgba16f(),
            w,
            h,
        );
        let data: Vec<u8> = image
            .rgb
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], 1.0])
            .flat_map(f32::to_le_bytes)
            .collect();
        ctx.texture_sub_image_2d(equirect, 0, 0, 0, w, h, &RGBA16F_FROM_F32, &data);
        drop(data);
        ctx.texture_generate_mipmaps(equirect);

        let source = cube_texture(
            ctx,
            size,
            mip_count(size),
            TextureMinFilter::LinearMipmapLinear,
        )?;
        let specular = cube_texture(
            ctx,
            size,
            SPECULAR_MIPS,
            TextureMinFilter::LinearMipmapLinear,
        )?;
        let diffuse = cube_texture(ctx, DIFFUSE_SIZE, 1, TextureMinFilter::Linear)?;
        let cleanup_on_error = |e: Error| {
            for t in [equirect, source, specular, diffuse] {
                ctx.destroy_texture(t);
            }
            e
        };

        // 1. Equirect → source cube, then its mips (the filtered sampling
        // below reads them).
        ctx.param_set_texture(self.group, self.f_equirect, equirect);
        self.faces(self.equirect, source, 0, size)
            .map_err(cleanup_on_error)?;
        ctx.texture_generate_mipmaps(source);

        // 2. Source → specular mips by roughness, and → diffuse irradiance.
        ctx.param_set_texture(self.group, self.f_source, source);
        ctx.param_set_f32(self.group, self.f_source_size, size as f32);
        for mip in 0..SPECULAR_MIPS {
            let roughness = mip as f32 / (SPECULAR_MIPS - 1) as f32;
            ctx.param_set_f32(self.group, self.f_roughness, roughness);
            self.faces(self.specular, specular, mip, (size >> mip).max(1))
                .map_err(cleanup_on_error)?;
        }
        self.faces(self.irradiance, diffuse, 0, DIFFUSE_SIZE)
            .map_err(cleanup_on_error)?;
        // Intermediates go; on an error above, `cleanup_on_error` took all four.
        ctx.destroy_texture(equirect);
        ctx.destroy_texture(source);

        Ok(Environment {
            diffuse,
            specular,
            specular_mips: SPECULAR_MIPS as u32,
        })
    }

    // Draws `program` into the six faces of `target` at `level` (`side`
    // texels square).
    fn faces(
        &self,
        program: ProgramId,
        target: TextureId,
        level: i32,
        side: i32,
    ) -> Result<(), Error> {
        let ctx = self.ctx;
        let state = ctx.render_state(&RenderState::new());
        for (i, face) in FACES.into_iter().enumerate() {
            ctx.framebuffer_attach_color_cube_face(self.fb, 0, target, face, level);
            if i == 0 && !ctx.is_framebuffer_complete(self.fb) {
                return Err(
                    "environment: this device can't render to half-float cube maps \
                     (no EXT_color_buffer_half_float / EXT_color_buffer_float)"
                        .into(),
                );
            }
            ctx.param_set_i32(self.group, self.f_face, i as i32);
            let mut pass = Pass::new(self.fb);
            pass.set_viewport(&Viewport::new(0, 0, side, side));
            let mut batch = Batch::new(program, state);
            batch.set_group(0, self.group);
            batch.draw_vertexless(&DrawCommand::arrays(DrawMode::Triangles, 0, 3));
            pass.add(&batch);
            ctx.render(&pass);
        }
        Ok(())
    }

    fn destroy(self) {
        let ctx = self.ctx;
        ctx.destroy_framebuffer(self.fb);
        ctx.destroy_param_group(self.group);
        ctx.destroy_param_layout(self.layout);
        for p in [self.equirect, self.specular, self.irradiance] {
            ctx.destroy_program(p);
        }
    }
}
