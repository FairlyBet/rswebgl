//! Reading an accessor as floats whatever its component type:
//! `KHR_mesh_quantization` stores positions, normals, tangents, texture
//! coordinates and morph deltas as (normalized or not) 8/16-bit integers,
//! which the `gltf` crate's float readers would misread. Normalized values
//! map as glTF 2.0 §3.11 says (signed: max(c / MAX, −1)); others convert
//! as they are. Sparse accessors are applied.

use gltf::accessor::DataType;

fn component(data: &[u8], at: usize, ty: DataType, normalized: bool) -> f32 {
    let b = |i: usize| data[at + i];
    match ty {
        DataType::I8 => {
            let c = b(0) as i8;
            if normalized {
                (f32::from(c) / 127.0).max(-1.0)
            } else {
                f32::from(c)
            }
        }
        DataType::U8 => {
            let c = b(0);
            if normalized {
                f32::from(c) / 255.0
            } else {
                f32::from(c)
            }
        }
        DataType::I16 => {
            let c = i16::from_le_bytes([b(0), b(1)]);
            if normalized {
                (f32::from(c) / 32767.0).max(-1.0)
            } else {
                f32::from(c)
            }
        }
        DataType::U16 => {
            let c = u16::from_le_bytes([b(0), b(1)]);
            if normalized {
                f32::from(c) / 65535.0
            } else {
                f32::from(c)
            }
        }
        DataType::U32 => u32::from_le_bytes([b(0), b(1), b(2), b(3)]) as f32,
        DataType::F32 => f32::from_le_bytes([b(0), b(1), b(2), b(3)]),
    }
}

fn read_index(data: &[u8], at: usize, ty: gltf::accessor::sparse::IndexType) -> usize {
    use gltf::accessor::sparse::IndexType as T;
    match ty {
        T::U8 => usize::from(data[at]),
        T::U16 => usize::from(u16::from_le_bytes([data[at], data[at + 1]])),
        T::U32 => u32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]]) as usize,
    }
}

fn view_bytes<'b>(view: &gltf::buffer::View, buffers: &'b [Vec<u8>]) -> Option<&'b [u8]> {
    let start = view.offset();
    buffers
        .get(view.buffer().index())?
        .get(start..start + view.length())
}

/// The accessor's elements as floats (`count × components`), or None when
/// its data is out of range. Without a bufferView, zeros (then sparse).
pub(crate) fn read_floats(a: &gltf::Accessor, buffers: &[Vec<u8>]) -> Option<Vec<f32>> {
    let n = a.dimensions().multiplicity();
    let ty = a.data_type();
    let size = ty.size();
    let count = a.count();
    let mut out = vec![0.0f32; count * n];
    if let Some(view) = a.view() {
        let data = view_bytes(&view, buffers)?;
        let stride = view.stride().unwrap_or(n * size);
        let offset = a.offset();
        if count > 0 && offset + (count - 1) * stride + n * size > data.len() {
            return None;
        }
        for i in 0..count {
            for c in 0..n {
                out[i * n + c] =
                    component(data, offset + i * stride + c * size, ty, a.normalized());
            }
        }
    }
    if let Some(sparse) = a.sparse() {
        let idx = sparse.indices();
        let vals = sparse.values();
        let idata = view_bytes(&idx.view(), buffers)?;
        let vdata = view_bytes(&vals.view(), buffers)?;
        let isize = match idx.index_type() {
            gltf::accessor::sparse::IndexType::U8 => 1,
            gltf::accessor::sparse::IndexType::U16 => 2,
            gltf::accessor::sparse::IndexType::U32 => 4,
        };
        for k in 0..sparse.count() {
            let iat = idx.offset() + k * isize;
            let vat = vals.offset() + k * n * size;
            if iat + isize > idata.len() || vat + n * size > vdata.len() {
                return None;
            }
            let i = read_index(idata, iat, idx.index_type());
            if i >= count {
                return None;
            }
            for c in 0..n {
                out[i * n + c] = component(vdata, vat + c * size, ty, a.normalized());
            }
        }
    }
    Some(out)
}

/// `read_floats` as `N`-component elements (None if it has another count).
pub(crate) fn read_vecs<const N: usize>(
    a: &gltf::Accessor,
    buffers: &[Vec<u8>],
) -> Option<Vec<[f32; N]>> {
    if a.dimensions().multiplicity() != N {
        return None;
    }
    let flat = read_floats(a, buffers)?;
    Some(flat.as_chunks::<N>().0.to_vec())
}
