// Sampler objects: override a texture's own parameters per group, don't leak
// into draws without one, are cached like texture bindings, and degrade to the
// texture's parameters when destroyed. Run with:
// wasm-pack test --headless --chrome packages/rswebgl
#![cfg(target_arch = "wasm32")]

mod common;

use wasm_bindgen_test::*;

use rswebgl::framebuffer::ClearMask;
use rswebgl::ids::{ParamGroupId, ProgramId, SamplerId, StateId, TextureId};
use rswebgl::param::ParamLayoutBuilder;
use rswebgl::pass::Pass;
use rswebgl::render_state::RenderState;
use rswebgl::sampler::SamplerDesc;
use rswebgl::texture::{
    TextureFormat, TextureMagFilter, TextureMinFilter, TextureTarget, TextureWrap,
};

use common::*;

wasm_bindgen_test_configure!(run_in_browser);

const NULL: ParamGroupId = ParamGroupId::NULL;

// Reads past the right edge: u = 1.25. CLAMP lands on the last texel, REPEAT
// wraps to u = 0.25 (the first).
const WRAP_FRAG: &str = r#"#version 300 es
precision highp float;
uniform sampler2D u_tex;
out vec4 frag_color;
void main() {
    frag_color = texture(u_tex, vec2(1.25, 0.5));
}
"#;

struct Setup {
    program: ProgramId,
    state: StateId,
    tex: TextureId,
    repeat: SamplerId,
}

// A 2×1 texture, red | green, NEAREST and CLAMP_TO_EDGE in its own parameters;
// a NEAREST + REPEAT sampler.
fn setup(f: &Fixture) -> Setup {
    let program = f.ctx.create_program(VERT, WRAP_FRAG).unwrap();
    let tex = f
        .ctx
        .create_texture(
            TextureTarget::Texture2D,
            TextureMinFilter::Nearest,
            TextureMagFilter::Nearest,
        )
        .unwrap();
    f.ctx
        .texture_storage_2d(tex, 1, &TextureFormat::rgba8(), 2, 1);
    let px = [RED, GREEN].concat();
    f.ctx
        .texture_sub_image_2d(tex, 0, 0, 0, 2, 1, &TextureFormat::rgba8(), &px);
    let mut desc = SamplerDesc::nearest_clamp();
    desc.wrap_s = TextureWrap::Repeat;
    let repeat = f.ctx.create_sampler(&desc).unwrap();
    Setup {
        program,
        state: f.ctx.render_state(&RenderState::new()),
        tex,
        repeat,
    }
}

fn group(f: &Fixture, s: &Setup, sampler: SamplerId) -> ParamGroupId {
    let layout = f
        .ctx
        .create_param_layout(&ParamLayoutBuilder::new().texture("u_tex"))
        .unwrap();
    let g = f.ctx.create_param_group(layout).unwrap();
    f.ctx.param_set_texture_by_name(g, "u_tex", s.tex);
    f.ctx.param_set_sampler_by_name(g, "u_tex", sampler);
    g
}

fn pass_of(f: &Fixture, s: &Setup, groups: [ParamGroupId; 3]) -> Pass {
    let mut pass = Pass::to_default();
    pass.set_clear(&ClearMask::color());
    for (col, g) in groups.into_iter().enumerate() {
        let q = quad(f, col as i32);
        pass.push(s.program, s.state, f.quad, g, NULL, NULL, q, &quad_cmd());
    }
    pass
}

// One texture read two ways in the same pass; the draw after the sampled one
// gets the texture's own parameters back.
#[wasm_bindgen_test]
fn sampler_overrides_texture_parameters_per_draw() {
    let f = fixture();
    let s = setup(&f);
    let own = group(&f, &s, SamplerId::NULL);
    let repeat = group(&f, &s, s.repeat);

    f.ctx.render(&pass_of(&f, &s, [own, repeat, own]));
    assert_eq!(read_quads(&f.gl), [GREEN, RED, GREEN]);
}

#[wasm_bindgen_test]
fn unchanged_sampler_is_not_rebound() {
    let f = fixture();
    let s = setup(&f);
    let repeat = group(&f, &s, s.repeat);
    let pass = pass_of(&f, &s, [repeat, repeat, repeat]);

    f.ctx.reset_frame_stats();
    f.ctx.render(&pass);
    assert_eq!(f.ctx.frame_stats().samplers, 1);
    f.ctx.reset_frame_stats();
    f.ctx.render(&pass);
    assert_eq!(f.ctx.frame_stats().samplers, 0);
    assert_eq!(read_quads(&f.gl), [RED; 3]);
}

// A destroyed sampler is reported, and the texture's own parameters apply.
#[wasm_bindgen_test]
fn destroyed_sampler_falls_back_to_texture_parameters() {
    let f = fixture();
    let s = setup(&f);
    let repeat = group(&f, &s, s.repeat);
    let pass = pass_of(&f, &s, [repeat, repeat, repeat]);
    f.ctx.render(&pass);
    assert_eq!(read_quads(&f.gl), [RED; 3]);

    f.ctx.destroy_sampler(s.repeat);
    assert!(!f.ctx.sampler_is_valid(s.repeat));
    let errors = capture_console("error", || f.ctx.render(&pass));
    assert!(errors.iter().any(|e| e.contains("sampler")), "{errors:?}");
    assert_eq!(read_quads(&f.gl), [GREEN; 3]);
}

#[wasm_bindgen_test]
fn sampler_desc_round_trips() {
    let f = fixture();
    let desc = SamplerDesc::trilinear_repeat();
    let id = f.ctx.create_sampler(&desc).unwrap();
    assert_eq!(f.ctx.sampler_desc(id), desc);

    // Anisotropy without the extension is ignored, with a warning.
    let mut aniso = SamplerDesc::trilinear_repeat();
    aniso.max_anisotropy = 8.0;
    let warnings = capture_console("warn", || {
        f.ctx.create_sampler(&aniso).unwrap();
    });
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("ExtTextureFilterAnisotropic")),
        "{warnings:?}"
    );
    assert_eq!(f.gl.get_error(), web_sys::WebGl2RenderingContext::NO_ERROR);
}

// The packer reads its sources through its own LINEAR sampler, whatever their
// own parameters (and without changing them): a 2×1 source that is NEAREST in
// its own parameters, packed down to 1×1, comes out as the average of its two
// texels rather than one of them.
#[wasm_bindgen_test]
fn packer_reads_sources_through_its_sampler() {
    use rswebgl::texture_packer::{Channel, PackSpec, TexturePacker};
    let f = fixture();
    let src = f
        .ctx
        .create_texture(
            TextureTarget::Texture2D,
            TextureMinFilter::Nearest,
            TextureMagFilter::Nearest,
        )
        .unwrap();
    f.ctx
        .texture_storage_2d(src, 1, &TextureFormat::rgba8(), 2, 1);
    let px = [[0, 0, 0, 255], [254, 0, 0, 255]].concat();
    f.ctx
        .texture_sub_image_2d(src, 0, 0, 0, 2, 1, &TextureFormat::rgba8(), &px);

    let packer = TexturePacker::new(&f.ctx).unwrap();
    let mut spec = PackSpec::new();
    spec.set_source(Channel::R, src, Channel::R);
    spec.set_constant(Channel::G, 0.0);
    spec.set_constant(Channel::B, 0.0);
    spec.set_constant(Channel::A, 1.0);
    let out = packer.pack(&spec, 1, 1, false, false).unwrap();

    // Show the output and read it back.
    let program = f.ctx.create_program(VERT, CENTER_FRAG).unwrap();
    let layout = f
        .ctx
        .create_param_layout(&ParamLayoutBuilder::new().texture("u_tex"))
        .unwrap();
    let g = f.ctx.create_param_group(layout).unwrap();
    f.ctx.param_set_texture_by_name(g, "u_tex", out);
    let mut pass = Pass::to_default();
    pass.set_clear(&ClearMask::color());
    let state = f.ctx.render_state(&RenderState::new());
    pass.push(
        program,
        state,
        f.quad,
        g,
        NULL,
        NULL,
        quad(&f, 1),
        &quad_cmd(),
    );
    f.ctx.render(&pass);
    let r = read_quads(&f.gl)[1][0];
    assert!(
        (120..=135).contains(&r),
        "red {r}: not the average of 0 and 254"
    );
}

const CENTER_FRAG: &str = r#"#version 300 es
precision highp float;
uniform sampler2D u_tex;
out vec4 frag_color;
void main() {
    frag_color = texture(u_tex, vec2(0.5));
}
"#;
