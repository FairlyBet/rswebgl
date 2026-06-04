//! Resolve every glTF `buffer` to its raw bytes.
//!
//! A buffer's data lives in one of three places (glTF 2.0 §3.6.1.2): the GLB
//! binary chunk (`Source::Bin`), a `data:` URI, or an external file referenced
//! by URI. The whole load is async, so external buffers are fetched on demand.

use base64::Engine;
use gltf::buffer::Source;
use wasm_bindgen::prelude::*;

use crate::fetch::{fetch_bytes, resolve_url};

/// One `Vec<u8>` per `document.buffers()`, in index order.
pub async fn resolve_buffers(
    doc: &gltf::Document,
    glb_blob: Option<Vec<u8>>,
    base: &str,
) -> Result<Vec<Vec<u8>>, JsValue> {
    let mut blob = glb_blob;
    let mut out = Vec::with_capacity(doc.buffers().count());
    for buffer in doc.buffers() {
        let data = match buffer.source() {
            // The GLB BIN chunk backs exactly the first buffer (index 0).
            Source::Bin => blob.take().ok_or_else(|| {
                JsValue::from_str("glTF references a GLB buffer but none present")
            })?,
            Source::Uri(uri) => match decode_data_uri(uri) {
                Some(bytes) => bytes?,
                None => {
                    let abs = resolve_url(base, uri)?;
                    fetch_bytes(&abs).await?
                }
            },
        };
        out.push(data);
    }
    Ok(out)
}

/// Decode a `data:` URI's payload. Returns `None` if `uri` isn't a data URI.
/// Handles both `;base64` and (rarely, for buffers) percent-less inline text.
pub fn decode_data_uri(uri: &str) -> Option<Result<Vec<u8>, JsValue>> {
    let rest = uri.strip_prefix("data:")?;
    let comma = match rest.find(',') {
        Some(c) => c,
        None => return Some(Err(JsValue::from_str("malformed data URI: no comma"))),
    };
    let meta = &rest[..comma];
    let payload = &rest[comma + 1..];
    if meta.contains(";base64") {
        Some(
            base64::engine::general_purpose::STANDARD
                .decode(payload)
                .map_err(|e| JsValue::from_str(&format!("data URI base64 decode: {e}"))),
        )
    } else {
        Some(Ok(payload.as_bytes().to_vec()))
    }
}

/// The MIME type declared in a `data:` image URI, if any (`data:image/png;base64,…`).
pub fn data_uri_mime(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("data:")?;
    let comma = rest.find(',')?;
    let meta = &rest[..comma];
    let mime = meta.split(';').next().unwrap_or("");
    (!mime.is_empty()).then(|| mime.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_base64_data_uri() {
        // "AAAA" base64 → three zero bytes.
        let got = decode_data_uri("data:application/octet-stream;base64,AAAA")
            .expect("is data URI")
            .expect("decodes");
        assert_eq!(got, vec![0, 0, 0]);
    }

    #[test]
    fn non_data_uri_is_none() {
        assert!(decode_data_uri("buffer.bin").is_none());
        assert!(decode_data_uri("https://example.com/x.bin").is_none());
    }

    #[test]
    fn reads_image_mime() {
        assert_eq!(
            data_uri_mime("data:image/png;base64,iVBOR"),
            Some("image/png".to_string())
        );
        assert_eq!(data_uri_mime("data:,plain"), None);
        assert_eq!(data_uri_mime("not-a-data-uri"), None);
    }
}
