// Host tests: `cargo test -p rswebgl-scene`.

use rswebgl_scene::{decode_hdr, encode_hdr};

fn header(w: u32, h: u32) -> Vec<u8> {
    format!("#?RADIANCE\nFORMAT=32-bit_rle_rgbe\nEXPOSURE=1.0\n\n-Y {h} +X {w}\n").into_bytes()
}

#[test]
fn flat_pixels_round_trip() {
    let rgb = [
        1.0, 0.5, 0.25, 100.0, 0.0, 3.0, 0.0, 0.0, 0.0, 0.001, 0.002, 0.003,
    ];
    let img = decode_hdr(&encode_hdr(2, 2, &rgb)).expect("decode");
    assert_eq!((img.width, img.height), (2, 2));
    for (got, want) in img.rgb.iter().zip(rgb) {
        // RGBE keeps 8 bits of mantissa against the brightest channel.
        assert!(
            (got - want).abs() <= want.abs() / 128.0 + 1e-3 * want.abs().max(0.01),
            "{got} vs {want}"
        );
    }
}

#[test]
fn new_style_rle_scanlines() {
    // 8 pixels wide: per channel, a run of 8 (R = 128), a literal run of 8
    // (G = 0..8), a run (B = 0), a run (E = 129: ×2⁻⁷).
    let mut bytes = header(8, 1);
    bytes.extend([2, 2, 0, 8]);
    bytes.extend([128 + 8, 128]);
    bytes.extend([8, 0, 1, 2, 3, 4, 5, 6, 7]);
    bytes.extend([128 + 8, 0]);
    bytes.extend([128 + 8, 129]);
    let img = decode_hdr(&bytes).expect("decode");
    let scale = 2f32.powi(129 - 136);
    for x in 0..8 {
        assert_eq!(img.rgb[x * 3], 128.0 * scale);
        assert_eq!(img.rgb[x * 3 + 1], x as f32 * scale);
        assert_eq!(img.rgb[x * 3 + 2], 0.0);
    }
}

#[test]
fn old_style_runs_repeat_the_last_pixel() {
    let mut bytes = header(5, 1);
    bytes.extend([64, 0, 0, 128]); // 0.25 red… (×2⁻⁸ · 64 = 0.25)
    bytes.extend([1, 1, 1, 4]); // …four more times
    let img = decode_hdr(&bytes).expect("decode");
    assert!(
        img.rgb.chunks(3).all(|p| p == [0.25, 0.0, 0.0]),
        "{:?}",
        img.rgb
    );
}

#[test]
fn bad_files_are_errors_not_panics() {
    assert!(decode_hdr(b"P6\n1 1\n255\n").is_err(), "not radiance");
    assert!(decode_hdr(b"#?RADIANCE\nFORMAT=32-bit_rle_xyze\n\n-Y 1 +X 1\n").is_err());
    assert!(
        decode_hdr(b"#?RADIANCE\n\n+Y 1 +X 1\n\0\0\0\0").is_err(),
        "orientation"
    );
    let mut short = header(4, 4);
    short.extend([1, 2, 3, 4]);
    assert!(decode_hdr(&short).is_err(), "truncated pixels");
    let mut runaway = header(8, 1);
    runaway.extend([2, 2, 0, 8, 128 + 100, 0]);
    assert!(decode_hdr(&runaway).is_err(), "run past the line");
    assert!(decode_hdr(b"#?RADIANCE").is_err(), "no header end");
}
