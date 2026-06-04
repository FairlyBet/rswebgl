//! Async resource fetching and URL resolution for the wasm/browser environment.
//!
//! The `gltf` crate's own `import` helpers are filesystem-based and unusable on
//! wasm, so we fetch buffers/images ourselves via `window.fetch` and resolve
//! relative URIs against the document base with the browser `URL` API.

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::Response;

/// `GET` a URL and return its bytes.
pub async fn fetch_bytes(url: &str) -> Result<Vec<u8>, JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let resp_value = JsFuture::from(window.fetch_with_str(url)).await?;
    let resp: Response = resp_value.dyn_into()?;
    if !resp.ok() {
        return Err(JsValue::from_str(&format!(
            "fetch \"{url}\" failed: HTTP {}",
            resp.status()
        )));
    }
    let buf = JsFuture::from(resp.array_buffer()?).await?;
    Ok(js_sys::Uint8Array::new(&buf).to_vec())
}

/// Turn the (possibly page-relative) `url` into an absolute URL, so it can serve
/// as a base for resolving the asset's external buffers/images.
pub fn absolute_base(url: &str) -> Result<String, JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let here = window.location().href()?;
    resolve_url(&here, url)
}

/// Resolve `rel` against absolute `base` (`new URL(rel, base)`).
pub fn resolve_url(base: &str, rel: &str) -> Result<String, JsValue> {
    let u = web_sys::Url::new_with_base(rel, base)
        .map_err(|_| JsValue::from_str(&format!("bad URL \"{rel}\" (base \"{base}\")")))?;
    Ok(u.href())
}
