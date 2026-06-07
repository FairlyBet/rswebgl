//! Pack several single-channel maps into the RGBA channels of one texture, on
//! the GPU.
//!
//! The classic use is an **ORM / ARM** texture: ambient-occlusion, roughness and
//! metallic each authored as a separate grayscale image, combined into one
//! `R=occlusion, G=roughness, B=metallic` texture so a material samples one
//! texture (and one texture unit) instead of three.
//!
//! Packing runs as a fullscreen blit into an FBO-attached target: each source is
//! sampled and the chosen channel written to the chosen output channel. Doing it
//! on the GPU (rather than reading pixels back to JS) keeps **linear data linear**
//! — no canvas colour-management/premultiply — and transparently resamples
//! sources of differing sizes to the output size.
//!
//! ```ignore
//! let packer = TexturePacker::new(&ctx)?;
//! let mut spec = PackSpec::new();
//! spec.set_source(Channel::R, &ao,  Channel::R); // occlusion -> R
//! spec.set_source(Channel::G, &mr,  Channel::G); // roughness -> G
//! spec.set_source(Channel::B, &mr,  Channel::B); // metallic  -> B
//! spec.set_constant(Channel::A, 1.0);
//! let orm = packer.pack(&spec, 1024, 1024, false, true)?;
//! ```

use wasm_bindgen::prelude::*;
use web_sys::WebGl2RenderingContext;

use crate::console;
use crate::context::Context;
use crate::draw::{DrawCommand, DrawMode, Viewport};
use crate::framebuffer::Framebuffer;
use crate::pass::{Batch, Pass};
use crate::program::Program;
use crate::render_state::RenderState;
use crate::renderer::Renderer;
use crate::texture::{
    Texture, TextureFormat, TextureMagFilter, TextureMinFilter, TextureTarget, mip_levels,
};
use crate::uniform_values::UniformValues;

/// A colour channel — used both to pick a source channel and to address an
/// output channel.
#[wasm_bindgen]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    R = 0,
    G = 1,
    B = 2,
    A = 3,
}

/// How to fill each output channel: read a channel of a source texture, or a
/// constant. Build it with `set_source` / `set_constant`; unset channels default
/// to a constant (`0` for R/G/B, `1` for A).
#[wasm_bindgen]
pub struct PackSpec {
    // Distinct source textures (max 4); `src_slot` indexes into this.
    sources: Vec<Texture>,
    mode: [i32; 4],     // per output channel: 0 = constant, 1 = source
    src_slot: [i32; 4], // per output channel: which source (index into `sources`)
    src_chan: [i32; 4], // per output channel: which channel of that source (0..3)
    consts: [f32; 4],   // per output channel: constant value (used when mode == 0)
}

impl Default for PackSpec {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl PackSpec {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            sources: Vec::new(),
            mode: [0; 4],
            src_slot: [0; 4],
            src_chan: [0; 4],
            // Sensible image default: opaque, black RGB.
            consts: [0.0, 0.0, 0.0, 1.0],
        }
    }

    /// Fill output channel `out` from channel `channel` of `tex`. Up to four
    /// distinct source textures may be referenced across all channels; a fifth is
    /// rejected with a logged error (the channel is left at its previous setting).
    pub fn set_source(&mut self, out: Channel, tex: &Texture, channel: Channel) {
        let slot = match self.sources.iter().position(|t| t == tex) {
            Some(i) => i,
            None => {
                if self.sources.len() >= 4 {
                    console::error(
                        "[rswebgl] PackSpec: a pack can reference at most 4 distinct source \
                         textures; ignoring extra source",
                    );
                    return;
                }
                self.sources.push(tex.clone());
                self.sources.len() - 1
            }
        };
        let o = out as usize;
        self.mode[o] = 1;
        self.src_slot[o] = slot as i32;
        self.src_chan[o] = channel as i32;
    }

    /// Fill output channel `out` with a constant value (overrides any source set
    /// on that channel).
    pub fn set_constant(&mut self, out: Channel, value: f32) {
        let o = out as usize;
        self.mode[o] = 0;
        self.consts[o] = value;
    }

    /// Number of distinct source textures referenced so far.
    pub fn source_count(&self) -> usize {
        self.sources.len()
    }
}

// Fullscreen triangle from gl_VertexID — no vertex buffer. uv spans [0,1] over
// the target, so the output texel at uv equals the source sampled at the same
// uv (an identity map: no flip, orientation preserved).
const PACK_VERT: &str = r#"#version 300 es
precision highp float;
out vec2 v_uv;
void main() {
    vec2 uv = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
    v_uv = uv;
    gl_Position = vec4(uv * 2.0 - 1.0, 0.0, 1.0);
}
"#;

// Per output channel i: a constant, or channel `u_out_chan[i]` of source slot
// `u_out_src[i]`. Sampler arrays can't be dynamically indexed in GLSL ES 3.00, so
// the four slots are separate samplers selected by a constant-indexed ladder;
// every slot is statically used (hence must be bound to a complete texture — the
// packer binds a 1×1 dummy to unused slots).
const PACK_FRAG: &str = r#"#version 300 es
precision highp float;
precision highp int;
in vec2 v_uv;
uniform sampler2D u_src0;
uniform sampler2D u_src1;
uniform sampler2D u_src2;
uniform sampler2D u_src3;
uniform int u_out_mode[4];
uniform int u_out_src[4];
uniform int u_out_chan[4];
uniform float u_out_const[4];
out vec4 frag_color;

vec4 sample_slot(int s, vec2 uv) {
    if (s == 0) return texture(u_src0, uv);
    if (s == 1) return texture(u_src1, uv);
    if (s == 2) return texture(u_src2, uv);
    return texture(u_src3, uv);
}

void main() {
    vec4 o = vec4(0.0);
    for (int i = 0; i < 4; i++) {
        if (u_out_mode[i] == 0) {
            o[i] = u_out_const[i];
        } else {
            o[i] = sample_slot(u_out_src[i], v_uv)[u_out_chan[i]];
        }
    }
    frag_color = o;
}
"#;

/// Reusable GPU channel packer: compiles the blit program and owns a scratch
/// framebuffer + dummy texture once, then packs any number of textures.
#[wasm_bindgen]
pub struct TexturePacker {
    gl: WebGl2RenderingContext,
    renderer: Renderer,
    program: Program,
    fbo: Framebuffer,
    // 1×1 complete texture bound to unused sampler slots so the draw is valid.
    dummy: Texture,
}

#[wasm_bindgen]
impl TexturePacker {
    /// Compile the packer's program and allocate its scratch resources.
    pub fn new(ctx: &Context) -> Result<TexturePacker, String> {
        let program = ctx.create_program(PACK_VERT, PACK_FRAG)?;
        // Size is irrelevant: the target is (re)attached per pack and the pass
        // viewport is set explicitly, so completeness follows the attachment.
        let fbo = ctx.create_framebuffer(1, 1)?;
        let gl = ctx.gl();
        let dummy = Texture::new(
            &gl,
            TextureTarget::Texture2D,
            TextureMinFilter::Nearest,
            TextureMagFilter::Nearest,
        )?;
        // texStorage zero-initialises, so the dummy reads (0,0,0,0); it only ever
        // backs slots no output channel samples.
        dummy.storage_2d(1, &TextureFormat::rgba8(), 1, 1);
        Ok(Self {
            gl,
            renderer: ctx.renderer(),
            program,
            fbo,
            dummy,
        })
    }

    /// Pack into a fresh `width × height` texture and return it. `srgb` selects
    /// the output internal format (`SRGB8_ALPHA8` vs `RGBA8`) — keep it `false`
    /// for data channels (occlusion/roughness/metallic), which are linear.
    pub fn pack(
        &self,
        spec: &PackSpec,
        width: i32,
        height: i32,
        srgb: bool,
        generate_mipmaps: bool,
    ) -> Result<Texture, String> {
        let (min, mag) = if generate_mipmaps {
            (
                TextureMinFilter::LinearMipmapLinear,
                TextureMagFilter::Linear,
            )
        } else {
            (TextureMinFilter::Linear, TextureMagFilter::Linear)
        };
        let out = Texture::new(&self.gl, TextureTarget::Texture2D, min, mag)?;
        self.pack_into(&out, spec, width, height, srgb, generate_mipmaps)?;
        Ok(out)
    }

    /// Pack into a caller-provided, *freshly created* texture (it allocates the
    /// storage). Useful when the output handle must exist before its size is known
    /// — e.g. an async loader that hands out the texture immediately and fills it
    /// once the sources decode.
    pub fn pack_into(
        &self,
        output: &Texture,
        spec: &PackSpec,
        width: i32,
        height: i32,
        srgb: bool,
        generate_mipmaps: bool,
    ) -> Result<(), String> {
        if width <= 0 || height <= 0 {
            return Err(format!("TexturePacker: invalid size {width}x{height}"));
        }

        let levels = if generate_mipmaps {
            mip_levels(width, height)
        } else {
            1
        };
        let format = if srgb {
            TextureFormat::srgb8_alpha8()
        } else {
            TextureFormat::rgba8()
        };
        output.storage_2d(levels, &format, width, height);

        self.fbo.attach_color_texture_2d(0, output, 0);
        // Detach before returning on any path, so the scratch FBO never pins the
        // produced texture alive past this call.
        let result = self.run_pass(spec, width, height);
        if result.is_ok() && generate_mipmaps {
            output.generate_mipmaps();
        }
        self.fbo.detach_color(0);
        result
    }
}

impl TexturePacker {
    fn run_pass(&self, spec: &PackSpec, width: i32, height: i32) -> Result<(), String> {
        self.renderer.check_framebuffer(&self.fbo)?;

        let uniforms = UniformValues::new();
        for i in 0..4 {
            let tex = spec.sources.get(i).unwrap_or(&self.dummy);
            uniforms.set_sampler(&format!("u_src{i}"), tex);
        }
        uniforms.set_int_array("u_out_mode", &spec.mode);
        uniforms.set_int_array("u_out_src", &spec.src_slot);
        uniforms.set_int_array("u_out_chan", &spec.src_chan);
        uniforms.set_float_array("u_out_const", &spec.consts);

        // The triangle covers the whole target, so no clear is needed. Default
        // render state (no depth/blend/cull, full colour mask) writes every texel.
        let state = RenderState::new();
        let mut batch = Batch::new(&self.program, &state);
        batch.draw_vertexless(&uniforms, DrawCommand::arrays(DrawMode::Triangles, 0, 3));

        let mut pass = Pass::new(&self.fbo);
        pass.set_viewport(Viewport::new(0, 0, width, height));
        pass.add(batch);
        self.renderer.render(&pass);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_discriminants() {
        assert_eq!(Channel::R as i32, 0);
        assert_eq!(Channel::G as i32, 1);
        assert_eq!(Channel::B as i32, 2);
        assert_eq!(Channel::A as i32, 3);
    }

    #[test]
    fn spec_defaults_to_opaque_black_constants() {
        let s = PackSpec::new();
        assert_eq!(s.mode, [0, 0, 0, 0]); // all constant
        assert_eq!(s.consts, [0.0, 0.0, 0.0, 1.0]); // RGB 0, A 1
        assert_eq!(s.source_count(), 0);
    }

    #[test]
    fn set_constant_overrides_channel() {
        let mut s = PackSpec::new();
        s.set_constant(Channel::G, 0.5);
        assert_eq!(s.mode[Channel::G as usize], 0);
        assert_eq!(s.consts[Channel::G as usize], 0.5);
    }
}
