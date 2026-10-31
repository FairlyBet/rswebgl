use rswebgl_foundation::layout::{FieldType, Scalar};
use rswebgl_foundation::pool::Destroyed;
use wasm_bindgen::prelude::*;
use web_sys::{WebGl2RenderingContext as Gl, WebGlProgram, WebGlShader, WebGlUniformLocation};

use crate::console;
use crate::context::{Context, stale};
use crate::extension::Extension;
use crate::ids::{ParamLayoutId, ProgramId};
use crate::limits;
use crate::param::{ParamGroupObj, ParamLayoutObj, PlainField};

const COMPLETION_STATUS_KHR: u32 = 0x91B1;

/// Debug checks a program runs while drawing (each reported once).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Validation {
    /// Uniform-block size vs the bound buffer's size, on first bind.
    pub blocks: bool,
    /// Active uniforms, samplers and blocks that none of a draw's groups sets.
    pub uniforms: bool,
}

// ---------------------------------------------------------------------------
// Reflection — what the linked program declares, read once.
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct ReflUniform {
    name: Box<str>,
    ty: FieldType,
    loc: WebGlUniformLocation,
    // Words in `Reflection::shadow` holding the last uploaded value.
    shadow: usize,
    words: usize,
    known: bool,
}

#[derive(Debug)]
struct ReflSampler {
    name: Box<str>,
    unit: u32,
    loc: WebGlUniformLocation,
}

#[derive(Debug)]
struct ReflBlock {
    name: Box<str>,
    point: u32,
    size: u32,
    size_checked: bool,
}

/// The program's active interface, plus the last value uploaded to each plain
/// uniform (so unchanged values aren't re-uploaded).
///
/// Texture units and uniform-buffer binding points are assigned here, one per
/// sampler / block in declaration order: the sampler's `uniform1i` is written
/// once on first use, `uniformBlockBinding` at link time.
#[derive(Debug)]
struct Reflection {
    uniforms: Vec<ReflUniform>,
    samplers: Vec<ReflSampler>,
    blocks: Vec<ReflBlock>,
    shadow: Vec<u32>,
    units_written: bool,
}

/// How one parameter layout's fields land in this program: the matching
/// uniform / sampler unit / block (None = the program doesn't use that name),
/// plus which of the program's inputs the layout covers.
#[derive(Debug)]
pub(crate) struct Mapping {
    layout: ParamLayoutId,
    pub(crate) plain: Vec<Option<u16>>,
    pub(crate) textures: Vec<Option<u32>>,
    pub(crate) blocks: Vec<Option<u16>>,
    coverage: Vec<u64>,
}

enum GlUniform {
    Value(FieldType),
    Sampler,
}

// A uniform's GL type as a field type (bool vectors upload like int vectors).
fn gl_uniform_type(t: u32) -> Option<GlUniform> {
    use FieldType::*;
    Some(GlUniform::Value(match t {
        Gl::FLOAT => F32,
        Gl::FLOAT_VEC2 => Vec2,
        Gl::FLOAT_VEC3 => Vec3,
        Gl::FLOAT_VEC4 => Vec4,
        Gl::INT => I32,
        Gl::INT_VEC2 | Gl::BOOL_VEC2 => IVec2,
        Gl::INT_VEC3 | Gl::BOOL_VEC3 => IVec3,
        Gl::INT_VEC4 | Gl::BOOL_VEC4 => IVec4,
        Gl::UNSIGNED_INT => U32,
        Gl::UNSIGNED_INT_VEC2 => UVec2,
        Gl::UNSIGNED_INT_VEC3 => UVec3,
        Gl::UNSIGNED_INT_VEC4 => UVec4,
        Gl::BOOL => Bool,
        Gl::FLOAT_MAT2 => Mat2,
        Gl::FLOAT_MAT3 => Mat3,
        Gl::FLOAT_MAT4 => Mat4,
        Gl::FLOAT_MAT2X3 => Mat2x3,
        Gl::FLOAT_MAT2X4 => Mat2x4,
        Gl::FLOAT_MAT3X2 => Mat3x2,
        Gl::FLOAT_MAT3X4 => Mat3x4,
        Gl::FLOAT_MAT4X2 => Mat4x2,
        Gl::FLOAT_MAT4X3 => Mat4x3,
        Gl::SAMPLER_2D
        | Gl::SAMPLER_3D
        | Gl::SAMPLER_CUBE
        | Gl::SAMPLER_2D_SHADOW
        | Gl::SAMPLER_2D_ARRAY
        | Gl::SAMPLER_2D_ARRAY_SHADOW
        | Gl::SAMPLER_CUBE_SHADOW
        | Gl::INT_SAMPLER_2D
        | Gl::INT_SAMPLER_3D
        | Gl::INT_SAMPLER_CUBE
        | Gl::INT_SAMPLER_2D_ARRAY
        | Gl::UNSIGNED_INT_SAMPLER_2D
        | Gl::UNSIGNED_INT_SAMPLER_3D
        | Gl::UNSIGNED_INT_SAMPLER_CUBE
        | Gl::UNSIGNED_INT_SAMPLER_2D_ARRAY => return Some(GlUniform::Sampler),
        _ => return None,
    }))
}

fn reflect(gl: &Gl, raw: &WebGlProgram) -> Reflection {
    let count = gl
        .get_program_parameter(raw, Gl::ACTIVE_UNIFORMS)
        .as_f64()
        .unwrap_or(0.0) as u32;
    let indices: js_sys::Array = (0..count).map(JsValue::from).collect();
    let block_of =
        js_sys::Array::from(&gl.get_active_uniforms(raw, &indices, Gl::UNIFORM_BLOCK_INDEX));

    let mut uniforms = Vec::new();
    let mut samplers = Vec::new();
    let mut words = 0usize;
    for i in 0..count {
        // Block members are set through the block's buffer, not one by one.
        if block_of.get(i).as_f64() != Some(-1.0) {
            continue;
        }
        let Some(info) = gl.get_active_uniform(raw, i) else {
            continue;
        };
        let full = info.name();
        if full.starts_with("gl_") {
            continue;
        }
        let name = full.strip_suffix("[0]").unwrap_or(&full).to_string();
        let Some(loc) = gl.get_uniform_location(raw, &name) else {
            continue;
        };
        match gl_uniform_type(info.type_()) {
            Some(GlUniform::Value(ty)) => {
                let w = (ty.components() * info.size().max(1) as u32) as usize;
                uniforms.push(ReflUniform {
                    name: name.into(),
                    ty,
                    loc,
                    shadow: words,
                    words: w,
                    known: false,
                });
                words += w;
            }
            Some(GlUniform::Sampler) => {
                if info.size() > 1 {
                    console::warn_once(&format!(
                        "[rswebgl] sampler arrays aren't supported yet: \"{name}\""
                    ));
                    continue;
                }
                let unit = samplers.len() as u32;
                if unit >= limits::max_combined_texture_units() {
                    console::warn_once(&format!(
                        "[rswebgl] sampler \"{name}\": all {} texture units are in use",
                        limits::max_combined_texture_units()
                    ));
                    continue;
                }
                samplers.push(ReflSampler {
                    name: name.into(),
                    unit,
                    loc,
                });
            }
            None => console::warn_once(&format!(
                "[rswebgl] uniform \"{name}\" has an unsupported type 0x{:X}",
                info.type_()
            )),
        }
    }

    let block_count = gl
        .get_program_parameter(raw, Gl::ACTIVE_UNIFORM_BLOCKS)
        .as_f64()
        .unwrap_or(0.0) as u32;
    let mut blocks = Vec::new();
    for index in 0..block_count {
        let Some(name) = gl.get_active_uniform_block_name(raw, index) else {
            continue;
        };
        let point = index;
        if point >= limits::max_uniform_buffer_bindings() {
            console::warn_once(&format!(
                "[rswebgl] uniform block \"{name}\": all {} binding points are in use",
                limits::max_uniform_buffer_bindings()
            ));
            continue;
        }
        gl.uniform_block_binding(raw, index, point);
        let size = gl
            .get_active_uniform_block_parameter(raw, index, Gl::UNIFORM_BLOCK_DATA_SIZE)
            .ok()
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0) as u32;
        blocks.push(ReflBlock {
            name: name.into(),
            point,
            size,
            size_checked: false,
        });
    }

    Reflection {
        uniforms,
        samplers,
        blocks,
        shadow: vec![0; words],
        units_written: false,
    }
}

// Uploads a plain uniform from 32-bit words (all of its array elements present
// in `words`).
fn upload(gl: &Gl, loc: &WebGlUniformLocation, ty: FieldType, words: &[u32]) {
    use FieldType::*;
    let loc = Some(loc);
    // SAFETY: u32/f32/i32 share size and alignment; any bits are valid.
    let f = unsafe { std::slice::from_raw_parts(words.as_ptr().cast::<f32>(), words.len()) };
    let i = unsafe { std::slice::from_raw_parts(words.as_ptr().cast::<i32>(), words.len()) };
    match ty {
        F32 => gl.uniform1fv_with_f32_array(loc, f),
        Vec2 => gl.uniform2fv_with_f32_array(loc, f),
        Vec3 => gl.uniform3fv_with_f32_array(loc, f),
        Vec4 => gl.uniform4fv_with_f32_array(loc, f),
        I32 | Bool => gl.uniform1iv_with_i32_array(loc, i),
        IVec2 => gl.uniform2iv_with_i32_array(loc, i),
        IVec3 => gl.uniform3iv_with_i32_array(loc, i),
        IVec4 => gl.uniform4iv_with_i32_array(loc, i),
        U32 => gl.uniform1uiv_with_u32_array(loc, words),
        UVec2 => gl.uniform2uiv_with_u32_array(loc, words),
        UVec3 => gl.uniform3uiv_with_u32_array(loc, words),
        UVec4 => gl.uniform4uiv_with_u32_array(loc, words),
        Mat2 => gl.uniform_matrix2fv_with_f32_array(loc, false, f),
        Mat3 => gl.uniform_matrix3fv_with_f32_array(loc, false, f),
        Mat4 => gl.uniform_matrix4fv_with_f32_array(loc, false, f),
        Mat2x3 => gl.uniform_matrix2x3fv_with_f32_array(loc, false, f),
        Mat2x4 => gl.uniform_matrix2x4fv_with_f32_array(loc, false, f),
        Mat3x2 => gl.uniform_matrix3x2fv_with_f32_array(loc, false, f),
        Mat3x4 => gl.uniform_matrix3x4fv_with_f32_array(loc, false, f),
        Mat4x2 => gl.uniform_matrix4x2fv_with_f32_array(loc, false, f),
        Mat4x3 => gl.uniform_matrix4x3fv_with_f32_array(loc, false, f),
    }
}

// ---------------------------------------------------------------------------
// ProgramObj
// ---------------------------------------------------------------------------

/// A program in the context's pool.
#[derive(Debug)]
pub(crate) struct ProgramObj {
    pub(crate) raw: WebGlProgram,
    validate: Validation,
    vert: Option<WebGlShader>,
    frag: Option<WebGlShader>,
    ready: bool,
    valid: bool,
    refl: Option<Reflection>,
    mappings: Vec<Mapping>,
    // Inputs (uniforms, then samplers, then blocks) already reported unset.
    reported_unset: Vec<bool>,
}

impl ProgramObj {
    fn new(
        gl: &Gl,
        vert_src: &str,
        frag_src: &str,
        parallel: bool,
        validate: Validation,
    ) -> Result<ProgramObj, String> {
        let vert = create_shader(gl, Gl::VERTEX_SHADER, vert_src)?;
        let frag = create_shader(gl, Gl::FRAGMENT_SHADER, frag_src)?;

        let raw = gl.create_program().ok_or("createProgram failed")?;
        gl.attach_shader(&raw, &vert);
        gl.attach_shader(&raw, &frag);
        gl.link_program(&raw);

        let mut program = ProgramObj {
            raw,
            validate,
            vert: Some(vert),
            frag: Some(frag),
            ready: false,
            valid: false,
            refl: None,
            mappings: Vec::new(),
            reported_unset: Vec::new(),
        };

        // Without parallel compile the first status query blocks on the link
        // anyway, so do it now and hand compile/link errors back to the caller.
        // With it, the link finishes in the background; `program_is_ready`
        // polls it without blocking, and the renderer waits on it at first draw.
        if !parallel && let Err(e) = program.finalize(gl) {
            program.delete(gl);
            return Err(e);
        }
        Ok(program)
    }

    pub(crate) fn delete(self, gl: &Gl) {
        gl.delete_shader(self.vert.as_ref());
        gl.delete_shader(self.frag.as_ref());
        gl.delete_program(Some(&self.raw));
    }

    // Waits for the link if it's still running (blocking), reporting a failure
    // once. Returns whether the program is usable.
    pub(crate) fn ensure_linked(&mut self, gl: &Gl) -> bool {
        if !self.ready
            && let Err(msg) = self.finalize(gl)
        {
            console::error(&format!("[rswebgl] {msg}"));
        }
        self.valid
    }

    // Reads the link status once the link has completed, reflects the program
    // if it linked, frees the shader objects, and returns the compile/link logs
    // on failure.
    fn finalize(&mut self, gl: &Gl) -> Result<(), String> {
        self.ready = true;
        self.valid = gl
            .get_program_parameter(&self.raw, Gl::LINK_STATUS)
            .as_bool()
            .unwrap_or(false);

        let result = if self.valid {
            let refl = reflect(gl, &self.raw);
            self.reported_unset =
                vec![false; refl.uniforms.len() + refl.samplers.len() + refl.blocks.len()];
            self.refl = Some(refl);
            Ok(())
        } else {
            let prog_log = gl
                .get_program_info_log(&self.raw)
                .unwrap_or_else(|| "unknown error".into());
            let mut msg = format!("program link failed: {}", prog_log.trim());
            for (stage, shader) in [("vertex", &self.vert), ("fragment", &self.frag)] {
                let log = shader
                    .as_ref()
                    .and_then(|s| gl.get_shader_info_log(s))
                    .unwrap_or_default();
                if !log.trim().is_empty() {
                    msg.push_str(&format!("\n{stage} shader: {}", log.trim()));
                }
            }
            Err(msg)
        };

        gl.delete_shader(self.vert.take().as_ref());
        gl.delete_shader(self.frag.take().as_ref());
        result
    }

    // Call right after the program is made current: writes each sampler's unit
    // (once per program lifetime).
    pub(crate) fn activate(&mut self, gl: &Gl) {
        if let Some(r) = &mut self.refl
            && !r.units_written
        {
            for s in &r.samplers {
                gl.uniform1i(Some(&s.loc), s.unit as i32);
            }
            r.units_written = true;
        }
    }

    /// Index of the mapping for `layout` (built on first use).
    pub(crate) fn mapping(&mut self, id: ParamLayoutId, layout: &ParamLayoutObj) -> usize {
        if let Some(i) = self.mappings.iter().position(|m| m.layout == id) {
            return i;
        }
        let refl = self.refl.as_ref().expect("mapping on a linked program");
        let n_u = refl.uniforms.len();
        let n_s = refl.samplers.len();
        let mut coverage = vec![0u64; (n_u + n_s + refl.blocks.len()).div_ceil(64)];
        let mut cover = |bit: usize| coverage[bit / 64] |= 1 << (bit % 64);

        let plain = layout
            .plain
            .iter()
            .map(|f| {
                let i = refl.uniforms.iter().position(|u| *u.name == *f.name)?;
                let u = &refl.uniforms[i];
                let compatible = u.ty == f.ty
                    || (u.ty == FieldType::Bool
                        && matches!(f.ty.scalar(), Scalar::Bool | Scalar::I32 | Scalar::U32));
                if !compatible {
                    console::warn_once(&format!(
                        "[rswebgl] param \"{}\" is {:?} in the layout but {:?} in the shader; \
                         not uploaded",
                        f.name, f.ty, u.ty
                    ));
                    return None;
                }
                cover(i);
                Some(i as u16)
            })
            .collect();
        let textures = layout
            .textures
            .iter()
            .map(|name| {
                let i = refl.samplers.iter().position(|s| *s.name == **name)?;
                cover(n_u + i);
                Some(refl.samplers[i].unit)
            })
            .collect();
        let blocks = layout
            .blocks
            .iter()
            .map(|name| {
                let i = refl.blocks.iter().position(|b| *b.name == **name)?;
                cover(n_u + n_s + i);
                Some(i as u16)
            })
            .collect();

        self.mappings.push(Mapping {
            layout: id,
            plain,
            textures,
            blocks,
            coverage,
        });
        self.mappings.len() - 1
    }

    pub(crate) fn mapping_at(&self, i: usize) -> &Mapping {
        &self.mappings[i]
    }

    /// Uploads plain field `field` of `group` (through mapping `m`) if it
    /// differs from what the program already holds; returns whether it did.
    pub(crate) fn upload_field(
        &mut self,
        gl: &Gl,
        m: usize,
        field: usize,
        f: &PlainField,
        group: &ParamGroupObj,
    ) -> bool {
        let Some(ui) = self.mappings[m].plain[field] else {
            return false;
        };
        let refl = self.refl.as_mut().expect("linked");
        let u = &mut refl.uniforms[ui as usize];
        // An array may be shorter in the shader (or the layout): send the overlap.
        let n = f.words.min(u.words);
        let src = &group.uints(f)[..n];
        let shadow = &mut refl.shadow[u.shadow..u.shadow + n];
        if u.known && shadow == src {
            return false;
        }
        upload(gl, &u.loc, u.ty, src);
        shadow.copy_from_slice(src);
        u.known = true;
        true
    }

    /// The binding point of block `i`, checking (once, if enabled) that the
    /// buffer bound to it matches the shader's block size.
    pub(crate) fn block_point(&mut self, i: u16, buffer_size: u32) -> u32 {
        let check = self.validate.blocks;
        let refl = self.refl.as_mut().expect("linked");
        let b = &mut refl.blocks[i as usize];
        if check && !b.size_checked {
            b.size_checked = true;
            if buffer_size != b.size {
                console::error(&format!(
                    "[rswebgl] uniform block \"{}\" layout mismatch: shader needs {} bytes, \
                     the buffer has {buffer_size}. Check field order/types/counts against the \
                     GLSL block.",
                    b.name, b.size
                ));
            }
        }
        b.point
    }

    // Reports (once each) inputs that none of `mappings` covers: such a
    // uniform keeps whatever an earlier draw left; a sampler reads whatever is
    // on its unit; a block reads whatever buffer is on its point.
    pub(crate) fn check_covered(&mut self, mappings: &[usize]) {
        if !self.validate.uniforms {
            return;
        }
        let Some(refl) = &self.refl else {
            return;
        };
        let n_u = refl.uniforms.len();
        let n_s = refl.samplers.len();
        for bit in 0..self.reported_unset.len() {
            if self.reported_unset[bit]
                || mappings
                    .iter()
                    .any(|&m| self.mappings[m].coverage[bit / 64] & (1 << (bit % 64)) != 0)
            {
                continue;
            }
            self.reported_unset[bit] = true;
            let (what, name) = if bit < n_u {
                ("uniform", &refl.uniforms[bit].name)
            } else if bit < n_u + n_s {
                ("sampler", &refl.samplers[bit - n_u].name)
            } else {
                ("uniform block", &refl.blocks[bit - n_u - n_s].name)
            };
            console::warn(&format!(
                "[rswebgl] {what} \"{name}\" is in none of the draw's parameter groups, so it \
                 keeps whatever an earlier draw left. Add it to a group's layout."
            ));
        }
    }

    fn active_names(&self) -> Vec<String> {
        let Some(r) = &self.refl else {
            return Vec::new();
        };
        let mut names: Vec<String> = r
            .uniforms
            .iter()
            .map(|u| u.name.to_string())
            .chain(r.samplers.iter().map(|s| s.name.to_string()))
            .collect();
        names.sort();
        names
    }
}

#[wasm_bindgen]
impl Context {
    /// Compiles and links a program. Without `KHR_parallel_shader_compile` a
    /// compile or link error comes back here, with the shader logs; with it,
    /// creation doesn't block and errors are reported when the link completes.
    pub fn create_program(&self, vert_src: &str, frag_src: &str) -> Result<ProgramId, String> {
        self.check_alive()?;
        let parallel = self.is_extension_enabled(Extension::KhrParallelShaderCompile);
        let obj = ProgramObj::new(
            self.gl(),
            vert_src,
            frag_src,
            parallel,
            self.core.validate.get(),
        )?;
        let mut res = self.res();
        match res.programs.insert(obj) {
            Ok(h) => Ok(h.into()),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn destroy_program(&self, program: ProgramId) {
        let destroyed = self.res().programs.destroy(program.0);
        match destroyed {
            Destroyed::Now(obj) => obj.delete(self.gl()),
            Destroyed::Deferred => {}
            Destroyed::Invalid => stale("program", "destroy_program", program),
        }
    }

    /// Whether the link has finished. Always true unless the program was created
    /// with `KHR_parallel_shader_compile` enabled; then it polls without blocking
    /// (the renderer would otherwise wait on the link at first draw), and a failed
    /// link is logged to the console once, when it completes. False for a stale
    /// handle.
    pub fn program_is_ready(&self, program: ProgramId) -> bool {
        let gl = self.gl();
        let mut res = self.res();
        let Some(p) = res.programs.get_mut(program.0) else {
            return false;
        };
        if p.ready {
            return true;
        }
        // A lost context reports every link as complete and failed.
        if gl.is_context_lost() {
            return false;
        }
        let complete = gl
            .get_program_parameter(&p.raw, COMPLETION_STATUS_KHR)
            .as_bool()
            .unwrap_or(false);
        if complete {
            p.ensure_linked(gl);
        }
        p.ready
    }

    /// Whether the program linked successfully (false while still linking, or
    /// for a stale handle).
    pub fn program_is_valid(&self, program: ProgramId) -> bool {
        self.res().programs.get(program.0).is_some_and(|p| p.valid)
    }

    /// Names of the program's active uniforms outside uniform blocks (samplers
    /// included; arrays as their base name, without `[0]`), sorted. Waits for
    /// the link if needed.
    pub fn program_active_uniform_names(&self, program: ProgramId) -> Vec<String> {
        let gl = self.gl();
        let mut res = self.res();
        let Some(p) = res.programs.get_mut(program.0) else {
            stale("program", "program_active_uniform_names", program);
            return Vec::new();
        };
        p.ensure_linked(gl);
        p.active_names()
    }
}

fn create_shader(gl: &Gl, shader_type: u32, src: &str) -> Result<WebGlShader, String> {
    let shader = gl.create_shader(shader_type).ok_or("createShader failed")?;
    gl.shader_source(&shader, src);
    gl.compile_shader(&shader);
    Ok(shader)
}
