//! Option objects from JS: plain objects read by key, with TypeScript types
//! for them in the generated `.d.ts`. A malformed object is an error the call
//! throws — options are setup, not per-frame data.

use glam::Vec3;
use js_sys::Reflect;
use rswebgl::error::Error;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(typescript_custom_section)]
const TS_OPTIONS: &str = r#"
/** A punctual light (names and units as in KHR_lights_punctual). */
export interface LightOptions {
  type: "directional" | "point" | "spot";
  /** Linear RGB; default white. */
  color?: ArrayLike<number>;
  /** Lux for directional lights, candela otherwise; default 1. */
  intensity?: number;
  /** Point / spot cutoff distance; 0 or absent = none. */
  range?: number;
  /** Spot cone, radians from the axis; defaults 0 and π/4. */
  innerConeAngle?: number;
  outerConeAngle?: number;
}

/** A camera. It looks down its entity's −Z, +Y up. */
export type CameraOptions =
  | { type: "perspective"; /** radians; default π/4 */ yfov?: number; znear?: number; zfar?: number }
  | { type: "orthographic"; /** half the view height; default 1 */ ymag?: number; znear?: number; zfar?: number };

/** How `loadTexture` makes a texture. */
export interface TextureOptions {
  /** "srgb" (default) for colors — baseColor, emissive; "linear" for data
   *  — normal, metallic-roughness, occlusion. */
  colorSpace?: "srgb" | "linear";
  /** Default "repeat". */
  wrap?: "repeat" | "clamp" | "mirror";
  /** Build mipmaps (trilinear filtering); default true. */
  mipmaps?: boolean;
  /** Flip rows on upload; default false (glTF's convention). */
  flipY?: boolean;
}

/** How `setMaterialTexture` uses a texture. */
export interface MaterialTextureOptions {
  /** Which TEXCOORD set the map reads: 0 (default) or 1. */
  texCoord?: number;
  /** KHR_texture_transform: UV offset (default [0, 0]). */
  offset?: [number, number];
  /** UV scale, i.e. repeats (default [1, 1]). */
  scale?: [number, number];
  /** UV rotation in radians (default 0). */
  rotation?: number;
}

/** What `pick` found. */
export interface PickHit {
  /** The mesh entity (its `parent` is the glTF node). */
  entity: number;
  /** World position hit. */
  point: Float32Array;
  /** From the camera. */
  distance: number;
}

/** What a mesh entity draws with: renderer handles, not owned by the scene. */
export interface MeshOptions {
  program: number;
  state: number;
  /** 0 or absent: no vertex array (vertex-pulling shaders). */
  vao?: number;
  /** Group slot 1; 0 or absent: none. */
  material?: number;
  /** Blended: drawn after opaque meshes, farthest first. */
  transparent?: boolean;
  /** Sees the opaque scene behind it (HDR output; scene_transmitted). */
  transmissive?: boolean;
}
"#;

fn get(obj: &JsValue, key: &str) -> Result<Option<JsValue>, Error> {
    // Absent options: every key takes its default.
    if obj.is_undefined() || obj.is_null() {
        return Ok(None);
    }
    if !obj.is_object() {
        return Err("expected an options object".into());
    }
    let v = Reflect::get(obj, &key.into()).map_err(|_| Error::from(format!("can't read {key}")))?;
    Ok((!v.is_undefined() && !v.is_null()).then_some(v))
}

pub(crate) fn f32_or(obj: &JsValue, key: &str, default: f32) -> Result<f32, Error> {
    match get(obj, key)? {
        None => Ok(default),
        Some(v) => v
            .as_f64()
            .map(|x| x as f32)
            .ok_or_else(|| format!("\"{key}\" must be a number").into()),
    }
}

pub(crate) fn u32_or(obj: &JsValue, key: &str, default: u32) -> Result<u32, Error> {
    let x = f32_or(obj, key, default as f32)?;
    if x < 0.0 || x.fract() != 0.0 {
        return Err(format!("\"{key}\" must be a handle (a non-negative integer)").into());
    }
    Ok(x as u32)
}

pub(crate) fn bool_or(obj: &JsValue, key: &str, default: bool) -> Result<bool, Error> {
    match get(obj, key)? {
        None => Ok(default),
        Some(v) => v
            .as_bool()
            .ok_or_else(|| format!("\"{key}\" must be true or false").into()),
    }
}

pub(crate) fn string(obj: &JsValue, key: &str) -> Result<String, Error> {
    get(obj, key)?
        .and_then(|v| v.as_string())
        .ok_or_else(|| format!("\"{key}\" must be a string").into())
}

pub(crate) fn vec3_or(obj: &JsValue, key: &str, default: Vec3) -> Result<Vec3, Error> {
    match get(obj, key)? {
        None => Ok(default),
        Some(v) => {
            let n =
                numbers(&v).ok_or_else(|| Error::from(format!("\"{key}\" must be 3 numbers")))?;
            match n.as_slice() {
                [x, y, z] => Ok(Vec3::new(*x, *y, *z)),
                _ => Err(format!("\"{key}\" must be 3 numbers").into()),
            }
        }
    }
}

/// A number or an array-like of numbers as floats (None for anything else).
pub(crate) fn numbers(v: &JsValue) -> Option<Vec<f32>> {
    if let Some(x) = v.as_f64() {
        return Some(vec![x as f32]);
    }
    if !v.is_object() {
        return None;
    }
    let len = Reflect::get(v, &"length".into()).ok()?.as_f64()? as u32;
    (0..len)
        .map(|i| Reflect::get_u32(v, i).ok()?.as_f64().map(|x| x as f32))
        .collect()
}

pub(crate) fn string_or(obj: &JsValue, key: &str, default: &str) -> Result<String, Error> {
    match get(obj, key)? {
        None => Ok(default.to_string()),
        Some(v) => v
            .as_string()
            .ok_or_else(|| format!("\"{key}\" must be a string").into()),
    }
}

// A pair of numbers at `key`, or `default` when absent.
pub(crate) fn vec2_or(obj: &JsValue, key: &str, default: [f32; 2]) -> Result<[f32; 2], Error> {
    match get(obj, key)? {
        None => Ok(default),
        Some(v) => match numbers(&v).as_deref() {
            Some(&[x, y]) => Ok([x, y]),
            _ => Err(format!("\"{key}\" needs 2 numbers").into()),
        },
    }
}
