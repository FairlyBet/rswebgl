// Image loads (`texture_load_bytes` / `texture_load_image`): pixels arrive
// as stored — not premultiplied, not color-converted — flipped only when
// asked, and failures call back with (0, 0). Run with:
// wasm-pack test --headless --chrome packages/rswebgl
#![cfg(target_arch = "wasm32")]

mod common;

use wasm_bindgen::prelude::*;
use wasm_bindgen_test::*;

use rswebgl::ids::TextureId;
use rswebgl::readback::ReadFormat;
use rswebgl::texture::{TextureFormat, TextureMagFilter, TextureMinFilter, TextureTarget};

use common::*;

wasm_bindgen_test_configure!(run_in_browser);

// The first row is the top of the image.
const TOP: [u8; 4] = [255, 0, 0, 255];
// Nearly transparent: premultiplying would drop green to ~10.
const BOTTOM: [u8; 4] = [0, 255, 0, 10];

// A 1×N RGBA PNG, rows top first, with stored (uncompressed) deflate.
fn png(rows: &[[u8; 4]]) -> Vec<u8> {
    fn crc32(data: &[u8]) -> u32 {
        let mut c = 0xffff_ffffu32;
        for &b in data {
            c ^= b as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xedb8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
        }
        !c
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8], data: &[u8]) {
        out.extend((data.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend(kind);
        out.extend(data);
        let crc = crc32(&out[start..]);
        out.extend(crc.to_be_bytes());
    }
    let raw: Vec<u8> = rows
        .iter()
        .flat_map(|p| [0, p[0], p[1], p[2], p[3]])
        .collect();
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    let len = raw.len() as u16;
    let mut zlib = vec![0x78, 0x01, 1];
    zlib.extend(len.to_le_bytes());
    zlib.extend((!len).to_le_bytes());
    zlib.extend(&raw);
    zlib.extend(((b << 16) | a).to_be_bytes());
    let mut ihdr = vec![0, 0, 0, 1];
    ihdr.extend((rows.len() as u32).to_be_bytes());
    ihdr.extend([8, 6, 0, 0, 0]);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &zlib);
    chunk(&mut out, b"IEND", &[]);
    out
}

fn new_texture(f: &Fixture) -> TextureId {
    f.ctx
        .create_texture(
            TextureTarget::Texture2D,
            TextureMinFilter::Nearest,
            TextureMagFilter::Nearest,
        )
        .unwrap()
}

// Resolves with the (width, height) `on_load` reports.
fn on_load() -> (js_sys::Function, js_sys::Promise) {
    let mut resolve = None;
    let promise = js_sys::Promise::new(&mut |r, _| resolve = Some(r));
    let resolve = resolve.unwrap();
    let cb = Closure::once_into_js(move |w: f64, h: f64| {
        let _ = resolve.call1(&JsValue::NULL, &js_sys::Array::of2(&w.into(), &h.into()));
    });
    (cb.unchecked_into(), promise)
}

async fn loaded(p: js_sys::Promise) -> (u32, u32) {
    let v: js_sys::Array = wasm_bindgen_futures::JsFuture::from(p)
        .await
        .unwrap()
        .into();
    (
        v.get(0).as_f64().unwrap() as u32,
        v.get(1).as_f64().unwrap() as u32,
    )
}

// Texture rows bottom (t = 0) first, as GL stores them.
fn texture_rows(f: &Fixture, t: TextureId, h: i32) -> Vec<[u8; 4]> {
    let fb = f.ctx.create_framebuffer(1, h).unwrap();
    f.ctx.framebuffer_attach_color_texture_2d(fb, 0, t, 0);
    let bytes = f
        .ctx
        .read_pixels(fb, 0, 0, 0, 1, h, ReadFormat::Rgba8)
        .unwrap();
    f.ctx.destroy_framebuffer(fb);
    bytes.as_chunks::<4>().0.to_vec()
}

async fn load_bytes(f: &Fixture, flip_y: bool) -> Vec<[u8; 4]> {
    let t = new_texture(f);
    let (cb, done) = on_load();
    let bytes = png(&[TOP, BOTTOM]);
    f.ctx.texture_load_bytes(
        t,
        &bytes,
        "image/png",
        &TextureFormat::rgba8(),
        false,
        flip_y,
        cb,
    );
    assert_eq!(loaded(done).await, (1, 2));
    texture_rows(f, t, 2)
}

#[wasm_bindgen_test]
async fn pixels_arrive_as_stored() {
    let f = fixture();
    // Without a flip the image's first (top) row lands at t = 0.
    assert_eq!(load_bytes(&f, false).await, [TOP, BOTTOM]);
    assert_eq!(load_bytes(&f, true).await, [BOTTOM, TOP]);
}

#[wasm_bindgen_test]
async fn mipmaps_are_generated_on_load() {
    let f = fixture();
    let t = new_texture(&f);
    let (cb, done) = on_load();
    let bytes = png(&[TOP, TOP, TOP, TOP]);
    f.ctx.texture_load_bytes(
        t,
        &bytes,
        "image/png",
        &TextureFormat::rgba8(),
        true,
        false,
        cb,
    );
    assert_eq!(loaded(done).await, (1, 4));
    let fb = f.ctx.create_framebuffer(1, 1).unwrap();
    f.ctx.framebuffer_attach_color_texture_2d(fb, 0, t, 2);
    assert_eq!(
        f.ctx
            .read_pixels(fb, 0, 0, 0, 1, 1, ReadFormat::Rgba8)
            .unwrap(),
        TOP
    );
}

#[wasm_bindgen_test]
async fn failed_loads_call_back_with_zero() {
    let f = fixture();
    let errors = js_sys::Array::new();
    let console = js_sys::Reflect::get(&js_sys::global(), &"console".into()).unwrap();
    let original = js_sys::Reflect::get(&console, &"error".into()).unwrap();
    let push = js_sys::Function::new_with_args("m", "this.push(String(m))").bind0(&errors);
    js_sys::Reflect::set(&console, &"error".into(), &push).unwrap();

    let (cb, done) = on_load();
    f.ctx.texture_load_image(
        new_texture(&f),
        "/no-such-image.png",
        &TextureFormat::rgba8(),
        false,
        false,
        cb,
    );
    let missing = loaded(done).await;
    let (cb, done) = on_load();
    f.ctx.texture_load_bytes(
        new_texture(&f),
        b"not an image",
        "image/png",
        &TextureFormat::rgba8(),
        false,
        false,
        cb,
    );
    let garbage = loaded(done).await;
    // Destroyed before it decodes: no report.
    let (cb, done) = on_load();
    let t = new_texture(&f);
    f.ctx.texture_load_bytes(
        t,
        &png(&[TOP]),
        "image/png",
        &TextureFormat::rgba8(),
        false,
        false,
        cb,
    );
    f.ctx.destroy_texture(t);
    let dropped = loaded(done).await;

    js_sys::Reflect::set(&console, &"error".into(), &original).unwrap();
    assert_eq!([missing, garbage, dropped], [(0, 0); 3]);
    let errors: Vec<String> = errors.iter().filter_map(|v| v.as_string()).collect();
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert!(errors[0].contains("no-such-image.png"), "{errors:?}");
}
