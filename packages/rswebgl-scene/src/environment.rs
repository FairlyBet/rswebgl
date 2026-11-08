//! Image-based lighting: an environment baked into the two cube maps the PBR
//! shader reads — diffuse irradiance and specular radiance prefiltered by
//! roughness (one mip per roughness step) — plus the BRDF lookup table they
//! share.
//!
//! Baking runs on the GPU (a few dozen full-screen draws into RGBA16F cube
//! faces, in stages a `PendingEnvironment` spreads over frames; the image
//! goes up as RGBE bytes and is decoded there), so it needs half-float render
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
use std::cell::RefCell;
use std::rc::Rc;

use crate::hdr::RgbeImage;

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

    /// Bakes an equirectangular HDR image (from `decode_hdr_rgbe`) right away,
    /// waiting for the bake shaders to link: a stall of tens of milliseconds
    /// (`PendingEnvironment` spreads it over frames instead). `size` as in
    /// `BakeJob::start`. Errors if the device can't render to half-float
    /// textures.
    pub fn bake(ctx: &Context, image: &RgbeImage, size: u32) -> Result<Self, Error> {
        let baker = EnvBaker::new(ctx)?;
        for p in baker.programs() {
            ctx.program_active_uniform_names(p); // waits for the link
        }
        let result = baker.check_linked().and_then(|()| {
            let mut job = BakeJob::start(ctx, image, size)?;
            loop {
                match job.step(&baker) {
                    Ok(Some(env)) => return Ok(env),
                    Ok(None) => {}
                    Err(e) => {
                        job.cancel();
                        return Err(e);
                    }
                }
            }
        });
        baker.destroy();
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
        .flat_map(|[a, b, sheen]| [a, b, sheen, 1.0])
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
/// The bake's shaders and parameter state, built once (compiled in the
/// background) and shared by every bake on a context.
pub struct EnvBaker {
    ctx: Context,
    destroyed: std::cell::Cell<bool>,
    decode: ProgramId,
    equirect: ProgramId,
    specular: ProgramId,
    irradiance: ProgramId,
    layout: ParamLayoutId,
    group: ParamGroupId,
    f_face: ParamField,
    f_roughness: ParamField,
    f_source_size: ParamField,
    f_rgbe: ParamField,
    f_equirect: ParamField,
    f_source: ParamField,
}

impl EnvBaker {
    /// Starts compiling the bake shaders; with `KHR_parallel_shader_compile`
    /// this doesn't wait for them.
    pub fn new(ctx: &Context) -> Result<Self, Error> {
        let program = |mode: &str| -> Result<ProgramId, Error> {
            let head = format!("#version 300 es\n#define {mode}\n");
            ctx.create_program(
                &format!("{head}{FULLSCREEN_VERT}"),
                &format!("{head}{BAKE_FRAG}"),
            )
        };
        let layout = ctx.create_param_layout(
            &ParamLayoutBuilder::new()
                .uniform("u_face", UniformType::I32)
                .uniform("u_roughness", UniformType::F32)
                .uniform("u_source_size", UniformType::F32)
                .texture("u_rgbe")
                .texture("u_equirect")
                .texture("u_source"),
        )?;
        Ok(Self {
            ctx: ctx.clone(),
            destroyed: std::cell::Cell::new(false),
            decode: program("DECODE")?,
            equirect: program("EQUIRECT")?,
            specular: program("SPECULAR")?,
            irradiance: program("IRRADIANCE")?,
            layout,
            group: ctx.create_param_group(layout)?,
            f_face: ctx.param_field(layout, "u_face")?,
            f_roughness: ctx.param_field(layout, "u_roughness")?,
            f_source_size: ctx.param_field(layout, "u_source_size")?,
            f_rgbe: ctx.param_field(layout, "u_rgbe")?,
            f_equirect: ctx.param_field(layout, "u_equirect")?,
            f_source: ctx.param_field(layout, "u_source")?,
        })
    }

    fn programs(&self) -> [ProgramId; 4] {
        [self.decode, self.equirect, self.specular, self.irradiance]
    }

    // Once linked: a bake program that failed would make every draw skip
    // and leave the cubes black while the bake "succeeds".
    fn check_linked(&self) -> Result<(), Error> {
        for p in self.programs() {
            if !self.ctx.program_is_valid(p) {
                return Err("environment: a bake shader failed to link".into());
            }
        }
        Ok(())
    }

    /// Frees the shaders (once; a bake still holding the baker stops at its
    /// next step if cancelled first).
    pub fn destroy(&self) {
        if self.destroyed.replace(true) {
            return;
        }
        let ctx = &self.ctx;
        ctx.destroy_param_group(self.group);
        ctx.destroy_param_layout(self.layout);
        for p in self.programs() {
            ctx.destroy_program(p);
        }
    }

    // One full-screen triangle of `program` into `fb` (`w`×`h`).
    fn draw(&self, program: ProgramId, fb: FramebufferId, w: i32, h: i32) {
        let ctx = &self.ctx;
        let mut pass = Pass::new(fb);
        pass.set_viewport(&Viewport::new(0, 0, w, h));
        let mut batch = Batch::new(program, ctx.render_state(&RenderState::new()));
        batch.set_group(0, self.group);
        batch.draw_vertexless(&DrawCommand::arrays(DrawMode::Triangles, 0, 3));
        pass.add(&batch);
        ctx.render(&pass);
    }

    // Draws `program` into the six faces of `target` at `level` (`side`
    // texels square).
    fn faces(
        &self,
        fb: FramebufferId,
        program: ProgramId,
        target: TextureId,
        level: i32,
        side: i32,
    ) -> Result<(), Error> {
        let ctx = &self.ctx;
        for (i, face) in FACES.into_iter().enumerate() {
            ctx.framebuffer_attach_color_cube_face(fb, 0, target, face, level);
            if i == 0 && !ctx.is_framebuffer_complete(fb) {
                return Err(HALF_FLOAT_TARGETS.into());
            }
            ctx.param_set_i32(self.group, self.f_face, i as i32);
            self.draw(program, fb, side, side);
        }
        Ok(())
    }
}

const HALF_FLOAT_TARGETS: &str = "environment: this device can't render to half-float \
     textures (no EXT_color_buffer_half_float / EXT_color_buffer_float)";

/// One bake's textures and progress. `start` uploads the image; each `step`
/// does one stage (a handful of draws) — decode, cube, each specular mip,
/// irradiance — so a bake can spread over frames.
pub struct BakeJob {
    ctx: Context,
    size: i32,
    width: i32,
    height: i32,
    rgbe: TextureId,
    equirect: TextureId,
    source: TextureId,
    specular: TextureId,
    diffuse: TextureId,
    fb: FramebufferId,
    stage: i32,
}

impl BakeJob {
    /// Uploads `image` (its RGBE bytes as they are: no conversion on the
    /// CPU) and makes the textures. `size` is the specular cube's face side
    /// (256 is plenty for lighting; the background shows it too), rounded up
    /// to a power of two in 32..=2048 (the specular mips need 32).
    pub fn start(ctx: &Context, image: &RgbeImage, size: u32) -> Result<Self, Error> {
        const MIN_SIZE: u32 = 1 << (SPECULAR_MIPS - 1);
        let size = size.clamp(MIN_SIZE, 2048).next_power_of_two() as i32;
        let (w, h) = (image.width as i32, image.height as i32);
        if image.rgbe.len() != (w * h * 4) as usize {
            return Err("environment: image data doesn't match its size".into());
        }
        let mut made: Vec<TextureId> = Vec::new();
        let result = Self::make(ctx, image, size, &mut made);
        if result.is_err() {
            for t in made {
                ctx.destroy_texture(t);
            }
        }
        result
    }

    fn make(
        ctx: &Context,
        image: &RgbeImage,
        size: i32,
        made: &mut Vec<TextureId>,
    ) -> Result<Self, Error> {
        use TextureMagFilter as Mag;
        use TextureMinFilter as Min;
        let (w, h) = (image.width as i32, image.height as i32);
        let mut texture = |target, min, levels, format: &TextureFormat, tw, th| {
            let t = ctx.create_texture(target, min, Mag::Linear)?;
            made.push(t);
            ctx.texture_storage_2d(t, levels, format, tw, th);
            Ok::<_, Error>(t)
        };
        let rgba16f = TextureFormat::rgba16f();
        // Read texel for texel by the decode pass: no filtering across
        // exponents.
        let rgbe = texture(
            TextureTarget::Texture2D,
            Min::Nearest,
            1,
            &TextureFormat::rgba8(),
            w,
            h,
        )?;
        ctx.texture_sub_image_2d(rgbe, 0, 0, 0, w, h, &TextureFormat::rgba8(), &image.rgbe);
        // The decoded image, mipmapped so the cube faces don't alias it.
        let mips = mip_count(w.max(h));
        let equirect = texture(
            TextureTarget::Texture2D,
            Min::LinearMipmapLinear,
            mips,
            &rgba16f,
            w,
            h,
        )?;
        ctx.texture_set_wrap_s(equirect, TextureWrap::Repeat);
        let source = texture(
            TextureTarget::TextureCubeMap,
            Min::LinearMipmapLinear,
            mip_count(size),
            &rgba16f,
            size,
            size,
        )?;
        let specular = texture(
            TextureTarget::TextureCubeMap,
            Min::LinearMipmapLinear,
            SPECULAR_MIPS,
            &rgba16f,
            size,
            size,
        )?;
        let diffuse = texture(
            TextureTarget::TextureCubeMap,
            Min::Linear,
            1,
            &rgba16f,
            DIFFUSE_SIZE,
            DIFFUSE_SIZE,
        )?;
        Ok(Self {
            ctx: ctx.clone(),
            size,
            width: w,
            height: h,
            rgbe,
            equirect,
            source,
            specular,
            diffuse,
            fb: ctx.create_framebuffer(w.max(size), h.max(size))?,
            stage: 0,
        })
    }

    /// Does the next stage; the environment once the last is done. After an
    /// error the job is spent (`cancel` it).
    pub fn step(&mut self, baker: &EnvBaker) -> Result<Option<Environment>, Error> {
        let ctx = &self.ctx;
        let stage = self.stage;
        self.stage += 1;
        match stage {
            // RGBE → linear half floats, then the mips the cube reads.
            0 => {
                ctx.framebuffer_attach_color_texture_2d(self.fb, 0, self.equirect, 0);
                if !ctx.is_framebuffer_complete(self.fb) {
                    return Err(HALF_FLOAT_TARGETS.into());
                }
                ctx.param_set_texture(baker.group, baker.f_rgbe, self.rgbe);
                baker.draw(baker.decode, self.fb, self.width, self.height);
                ctx.texture_generate_mipmaps(self.equirect);
                ctx.destroy_texture(std::mem::replace(&mut self.rgbe, TextureId::NULL));
            }
            // Equirect → source cube, then its mips (the filtered sampling of
            // the next stages reads them).
            1 => {
                ctx.param_set_texture(baker.group, baker.f_equirect, self.equirect);
                baker.faces(self.fb, baker.equirect, self.source, 0, self.size)?;
                ctx.texture_generate_mipmaps(self.source);
                ctx.destroy_texture(std::mem::replace(&mut self.equirect, TextureId::NULL));
            }
            // Source → the specular mip for roughness mip / (mips − 1).
            s if s < 2 + SPECULAR_MIPS => {
                let mip = s - 2;
                ctx.param_set_texture(baker.group, baker.f_source, self.source);
                ctx.param_set_f32(baker.group, baker.f_source_size, self.size as f32);
                let roughness = mip as f32 / (SPECULAR_MIPS - 1) as f32;
                ctx.param_set_f32(baker.group, baker.f_roughness, roughness);
                let side = (self.size >> mip).max(1);
                baker.faces(self.fb, baker.specular, self.specular, mip, side)?;
            }
            // Source → diffuse irradiance; done.
            _ => {
                ctx.param_set_texture(baker.group, baker.f_source, self.source);
                ctx.param_set_f32(baker.group, baker.f_source_size, self.size as f32);
                baker.faces(self.fb, baker.irradiance, self.diffuse, 0, DIFFUSE_SIZE)?;
                ctx.destroy_texture(std::mem::replace(&mut self.source, TextureId::NULL));
                ctx.destroy_framebuffer(std::mem::replace(&mut self.fb, FramebufferId::NULL));
                return Ok(Some(Environment {
                    diffuse: std::mem::replace(&mut self.diffuse, TextureId::NULL),
                    specular: std::mem::replace(&mut self.specular, TextureId::NULL),
                    specular_mips: SPECULAR_MIPS as u32,
                }));
            }
        }
        Ok(None)
    }

    /// Frees whatever the job still holds (all of it before the last step).
    pub fn cancel(self) {
        let ctx = &self.ctx;
        for t in [
            self.rgbe,
            self.equirect,
            self.source,
            self.specular,
            self.diffuse,
        ] {
            if !t.is_null() {
                ctx.destroy_texture(t);
            }
        }
        if !self.fb.is_null() {
            ctx.destroy_framebuffer(self.fb);
        }
    }
}

enum PendingState {
    Running,
    Done(Result<Environment, Error>),
    Over,
}

/// A bake running in the background: its own task does one stage per
/// frame or so (`STEP_MS` apart, on the context's poll timer), so loading an
/// environment doesn't stall rendering. `take` hands the result over once
/// it's done.
pub struct PendingEnvironment {
    ctx: Context,
    state: Rc<RefCell<PendingState>>,
}

// Spacing of the stages: about one per 60 Hz frame.
const STEP_MS: f64 = 12.0;

impl PendingEnvironment {
    /// Uploads `image` now and bakes it in the background with `baker`
    /// (after its shaders link).
    pub fn start(
        ctx: &Context,
        baker: Rc<EnvBaker>,
        image: &RgbeImage,
        size: u32,
    ) -> Result<Self, Error> {
        let mut job = BakeJob::start(ctx, image, size)?;
        let state = Rc::new(RefCell::new(PendingState::Running));
        let shared = state.clone();
        let task_ctx = ctx.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let ctx = task_ctx;
            ctx.programs_ready(baker.programs().to_vec()).await;
            let mut result = baker.check_linked().map(|()| None);
            while let Ok(None) = result {
                let running = matches!(*shared.borrow(), PendingState::Running);
                if !running || ctx.is_lost() {
                    job.cancel();
                    *shared.borrow_mut() = PendingState::Over;
                    return;
                }
                result = job.step(&baker);
                if let Ok(None) = result {
                    let last = js_sys::Date::now();
                    while js_sys::Date::now() - last < STEP_MS {
                        ctx.next_poll_round().await;
                    }
                }
            }
            let result = match result {
                Ok(env) => Ok(env.expect("the loop ends with an environment")),
                Err(e) => {
                    job.cancel();
                    Err(e)
                }
            };
            let mut state = shared.borrow_mut();
            if matches!(*state, PendingState::Running) {
                *state = PendingState::Done(result);
            } else if let Ok(env) = result {
                env.destroy(&ctx); // cancelled meanwhile
            }
        });
        Ok(Self {
            ctx: ctx.clone(),
            state,
        })
    }

    /// The result, once (None while running, and after it was taken).
    pub fn take(&self) -> Option<Result<Environment, Error>> {
        let mut state = self.state.borrow_mut();
        if !matches!(*state, PendingState::Done(_)) {
            return None;
        }
        match std::mem::replace(&mut *state, PendingState::Over) {
            PendingState::Done(result) => Some(result),
            _ => None,
        }
    }

    /// Resolves once the bake has finished (or was cancelled).
    pub fn finished(&self) -> impl std::future::Future<Output = ()> + use<> {
        let state = self.state.clone();
        let ctx = self.ctx.clone();
        async move {
            while matches!(*state.borrow(), PendingState::Running) {
                ctx.next_poll_round().await;
            }
        }
    }

    /// Stops the bake (its task frees what it made); a result not yet taken
    /// is destroyed.
    pub fn cancel(&self) {
        let old = std::mem::replace(&mut *self.state.borrow_mut(), PendingState::Over);
        if let PendingState::Done(Ok(env)) = old {
            env.destroy(&self.ctx);
        }
    }
}
