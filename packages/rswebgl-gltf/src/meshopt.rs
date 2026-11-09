//! Decoders for `EXT_meshopt_compression` / `KHR_meshopt_compression`
//! bufferViews — ported from meshoptimizer (MIT, Copyright (c) 2016-2025
//! Arseny Kapoulkine): `src/vertexcodec.cpp` (attributes, bitstream v0 and
//! v1), `src/indexcodec.cpp` (triangles, indices) and `src/vertexfilter.cpp`
//! (octahedral, quaternion, exponential and color filters), the scalar
//! paths, checked against the extension's bitstream appendix and
//! meshoptimizer's `demo/tests.cpp` vectors (below).

// Index loops and fixed-size chunks kept as the reference writes them.
#![allow(clippy::needless_range_loop, clippy::chunks_exact_to_as_chunks)]

/// What a compressed bufferView holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Attributes,
    Triangles,
    Indices,
}

/// The post-decoding transform of attribute data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    None,
    Octahedral,
    Quaternion,
    Exponential,
    Color,
}

/// Decodes `source` (`count` elements of `stride` bytes) into `out`
/// (`count × stride` bytes).
pub fn decode(
    out: &mut [u8],
    count: usize,
    stride: usize,
    source: &[u8],
    mode: Mode,
    filter: Filter,
) -> Result<(), String> {
    if out.len() < count * stride {
        return Err("meshopt: the target bufferView is too small".into());
    }
    let out = &mut out[..count * stride];
    match mode {
        Mode::Attributes => decode_vertex_buffer(out, count, stride, source)?,
        Mode::Triangles => decode_index_buffer(out, count, stride, source)?,
        Mode::Indices => decode_index_sequence(out, count, stride, source)?,
    }
    match filter {
        Filter::None => {}
        Filter::Octahedral if stride == 4 => filter_oct8(out),
        Filter::Octahedral if stride == 8 => filter_oct16(out),
        Filter::Quaternion if stride == 8 => filter_quat(out),
        Filter::Exponential if stride.is_multiple_of(4) => filter_exp(out),
        Filter::Color if stride == 4 => filter_color8(out),
        Filter::Color if stride == 8 => filter_color16(out),
        f => return Err(format!("meshopt: filter {f:?} with byteStride {stride}")),
    }
    Ok(())
}

// --- Attributes (vertexcodec.cpp) -------------------------------------------

const VERTEX_HEADER: u8 = 0xa0;
const VERTEX_BLOCK_SIZE_BYTES: usize = 8192;
const VERTEX_BLOCK_MAX_SIZE: usize = 256;
const BYTE_GROUP_SIZE: usize = 16;
const BYTE_GROUP_DECODE_LIMIT: usize = 24;
const TAIL_MIN_SIZE_V0: usize = 32;
const TAIL_MIN_SIZE_V1: usize = 24;
const BITS_V0: [u32; 4] = [0, 2, 4, 8];
const BITS_V1: [u32; 5] = [0, 1, 2, 4, 8];

fn vertex_block_size(vertex_size: usize) -> usize {
    let result = (VERTEX_BLOCK_SIZE_BYTES / vertex_size) & !(BYTE_GROUP_SIZE - 1);
    result.min(VERTEX_BLOCK_MAX_SIZE)
}

fn bad() -> String {
    "meshopt: malformed attribute data".into()
}

// One group of 16 deltas at `bits` each (sentinel-escaped below 8); the
// position after it.
fn decode_bytes_group(data: &[u8], mut pos: usize, out: &mut [u8], bits: u32) -> usize {
    match bits {
        0 => {
            out[..BYTE_GROUP_SIZE].fill(0);
            pos
        }
        8 => {
            out[..BYTE_GROUP_SIZE].copy_from_slice(&data[pos..pos + BYTE_GROUP_SIZE]);
            pos + BYTE_GROUP_SIZE
        }
        _ => {
            let per_byte = 8 / bits as usize;
            let packed = BYTE_GROUP_SIZE / per_byte;
            let mut var = pos + packed;
            let sentinel = (1u32 << bits) - 1;
            let mut o = 0;
            for _ in 0..packed {
                let mut byte = data[pos];
                pos += 1;
                // 1-bit groups are stored least significant bit first.
                if bits == 1 {
                    byte = byte.reverse_bits();
                }
                for _ in 0..per_byte {
                    let enc = u32::from(byte) >> (8 - bits);
                    byte = ((u32::from(byte) << bits) & 0xff) as u8;
                    if enc == sentinel {
                        out[o] = data[var];
                        var += 1;
                    } else {
                        out[o] = enc as u8;
                    }
                    o += 1;
                }
            }
            var
        }
    }
}

fn decode_bytes(data: &[u8], mut pos: usize, out: &mut [u8], bits: &[u32]) -> Option<usize> {
    let size = out.len();
    let header_size = (size / BYTE_GROUP_SIZE).div_ceil(4);
    if data.len() - pos < header_size {
        return None;
    }
    let header = pos;
    pos += header_size;
    for i in (0..size).step_by(BYTE_GROUP_SIZE) {
        if data.len() - pos < BYTE_GROUP_DECODE_LIMIT {
            return None;
        }
        let offset = i / BYTE_GROUP_SIZE;
        let bitsk = (data[header + offset / 4] >> ((offset % 4) * 2)) & 3;
        pos = decode_bytes_group(data, pos, &mut out[i..], bits[bitsk as usize]);
    }
    Some(pos)
}

// Byte-transposed deltas of one 4-byte channel → the elements, as `width`
// (1, 2 or 4) byte values: zigzag deltas, or (xor) rotated xors.
#[allow(clippy::too_many_arguments)]
fn decode_deltas4(
    buffer: &[u8],
    target: &mut [u8],
    k: usize,
    count: usize,
    vertex_size: usize,
    last: &[u8],
    width: usize,
    xor_rot: Option<u32>,
) {
    let mut buffer = buffer;
    let mut last = last;
    let mask: u64 = if width == 4 {
        0xffff_ffff
    } else {
        (1u64 << (8 * width)) - 1
    };
    for kk in (0..4).step_by(width) {
        let mut p: u64 = 0;
        for j in 0..width {
            p |= u64::from(last[j]) << (8 * j);
        }
        let mut offset = k + kk;
        for i in 0..count {
            let mut v: u64 = 0;
            for j in 0..width {
                v |= u64::from(buffer[i + count * j]) << (8 * j);
            }
            v = match xor_rot {
                Some(rot) => u64::from((v as u32).rotate_left(rot)) ^ p,
                None => ((0u64.wrapping_sub(v & 1)) ^ (v >> 1)).wrapping_add(p) & mask,
            } & mask;
            for j in 0..width {
                target[offset + j] = (v >> (j * 8)) as u8;
            }
            p = v;
            offset += vertex_size;
        }
        buffer = &buffer[count * width..];
        last = &last[width..];
    }
}

#[allow(clippy::too_many_arguments)]
fn decode_vertex_block(
    data: &[u8],
    mut pos: usize,
    out: &mut [u8],
    count: usize,
    vertex_size: usize,
    last: &mut [u8; 256],
    channels: Option<&[u8]>,
    version: u8,
) -> Option<usize> {
    let mut buffer = [0u8; VERTEX_BLOCK_MAX_SIZE * 4];
    let aligned = count.div_ceil(BYTE_GROUP_SIZE) * BYTE_GROUP_SIZE;
    let control_size = if version == 0 { 0 } else { vertex_size / 4 };
    if data.len() - pos < control_size {
        return None;
    }
    let control = pos;
    pos += control_size;
    for k in (0..vertex_size).step_by(4) {
        let ctrl_byte = if version == 0 {
            0
        } else {
            data[control + k / 4]
        };
        for j in 0..4 {
            let ctrl = (ctrl_byte >> (j * 2)) & 3;
            let dst = &mut buffer[j * count..j * count + aligned];
            match ctrl {
                3 => {
                    // literal
                    if data.len() - pos < count {
                        return None;
                    }
                    dst[..count].copy_from_slice(&data[pos..pos + count]);
                    pos += count;
                }
                2 => dst.fill(0),
                _ => {
                    let bits: &[u32] = if version == 0 {
                        &BITS_V0
                    } else {
                        &BITS_V1[ctrl as usize..]
                    };
                    pos = decode_bytes(data, pos, dst, bits)?;
                }
            }
        }
        let channel = channels.map_or(0, |c| c[k / 4]);
        let (width, xor) = match channel & 3 {
            0 => (1, None),
            1 => (2, None),
            2 => (4, Some((32 - u32::from(channel >> 4)) & 31)),
            _ => return None,
        };
        decode_deltas4(&buffer, out, k, count, vertex_size, &last[k..], width, xor);
    }
    last[..vertex_size].copy_from_slice(&out[vertex_size * (count - 1)..vertex_size * count]);
    Some(pos)
}

/// `meshopt_decodeVertexBuffer`.
pub fn decode_vertex_buffer(
    out: &mut [u8],
    count: usize,
    vertex_size: usize,
    data: &[u8],
) -> Result<(), String> {
    if vertex_size == 0 || vertex_size > 256 || !vertex_size.is_multiple_of(4) {
        return Err(format!("meshopt: byteStride {vertex_size} for attributes"));
    }
    let Some(&header) = data.first() else {
        return Err(bad());
    };
    if header & 0xf0 != VERTEX_HEADER || header & 0x0f > 1 {
        return Err("meshopt: unsupported attribute encoding".into());
    }
    let version = header & 0x0f;
    let tail_size = vertex_size + if version == 0 { 0 } else { vertex_size / 4 };
    let tail_min = if version == 0 {
        TAIL_MIN_SIZE_V0
    } else {
        TAIL_MIN_SIZE_V1
    };
    let tail_pad = tail_size.max(tail_min);
    if data.len() - 1 < tail_pad {
        return Err(bad());
    }
    let tail = data.len() - tail_size;
    let mut last = [0u8; 256];
    last[..vertex_size].copy_from_slice(&data[tail..tail + vertex_size]);
    let channels = (version != 0).then(|| &data[tail + vertex_size..]);
    let block = vertex_block_size(vertex_size);
    let mut pos = 1;
    let mut offset = 0;
    while offset < count {
        let n = block.min(count - offset);
        let dst = &mut out[offset * vertex_size..(offset + n) * vertex_size];
        pos = decode_vertex_block(data, pos, dst, n, vertex_size, &mut last, channels, version)
            .ok_or_else(bad)?;
        offset += n;
    }
    if data.len() - pos != tail_pad {
        return Err(bad());
    }
    Ok(())
}

// --- Indices (indexcodec.cpp) -------------------------------------------------

const INDEX_HEADER: u8 = 0xe0;
const SEQUENCE_HEADER: u8 = 0xd0;

fn decode_vbyte(data: &[u8], pos: &mut usize) -> u32 {
    let lead = data[*pos];
    *pos += 1;
    if lead < 128 {
        return u32::from(lead);
    }
    let mut result = u32::from(lead & 127);
    let mut shift = 7;
    for _ in 0..4 {
        let group = data[*pos];
        *pos += 1;
        result |= u32::from(group & 127) << shift;
        shift += 7;
        if group < 128 {
            break;
        }
    }
    result
}

fn decode_index(data: &[u8], pos: &mut usize, last: u32) -> u32 {
    let v = decode_vbyte(data, pos);
    let d = (v >> 1) ^ 0u32.wrapping_sub(v & 1);
    last.wrapping_add(d)
}

fn push_edge(fifo: &mut [[u32; 2]; 16], offset: &mut usize, a: u32, b: u32) {
    fifo[*offset] = [a, b];
    *offset = (*offset + 1) & 15;
}

fn write_index(out: &mut [u8], i: usize, size: usize, v: u32) {
    if size == 2 {
        out[i * 2..i * 2 + 2].copy_from_slice(&(v as u16).to_le_bytes());
    } else {
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
}

/// `meshopt_decodeIndexBuffer`: a triangle list of `count` indices.
pub fn decode_index_buffer(
    out: &mut [u8],
    count: usize,
    index_size: usize,
    buffer: &[u8],
) -> Result<(), String> {
    let malformed = || "meshopt: malformed triangle data".to_string();
    if !count.is_multiple_of(3) || !(index_size == 2 || index_size == 4) {
        return Err("meshopt: triangles need a multiple of 3 indices of 2 or 4 bytes".into());
    }
    if buffer.len() < 1 + count / 3 + 16 {
        return Err(malformed());
    }
    if buffer[0] & 0xf0 != INDEX_HEADER || buffer[0] & 0x0f > 1 {
        return Err("meshopt: unsupported triangle encoding".into());
    }
    let version = buffer[0] & 0x0f;
    let mut edgefifo = [[u32::MAX; 2]; 16];
    let mut vertexfifo = [u32::MAX; 16];
    let (mut edge_off, mut vertex_off) = (0usize, 0usize);
    let (mut next, mut last) = (0u32, 0u32);
    let fecmax = if version >= 1 { 13 } else { 15 };
    let code_end = 1 + count / 3;
    let mut data = code_end;
    // Each triangle reads at most 16 bytes, so reading up to the table is safe.
    let data_safe_end = buffer.len() - 16;
    let codeaux_table = data_safe_end;
    let mut tri = 0;
    let mut emit = |out: &mut [u8], a: u32, b: u32, c: u32| {
        write_index(out, tri * 3, index_size, a);
        write_index(out, tri * 3 + 1, index_size, b);
        write_index(out, tri * 3 + 2, index_size, c);
        tri += 1;
    };
    for code in 1..code_end {
        let codetri = buffer[code];
        let push_vertex = |f: &mut [u32; 16], off: &mut usize, v: u32, cond: bool| {
            f[*off] = v;
            *off = (*off + usize::from(cond)) & 15;
        };
        if codetri < 0xf0 {
            let fe = usize::from(codetri >> 4);
            let [a, b] = edgefifo[(edge_off.wrapping_sub(1 + fe)) & 15];
            let fec = i32::from(codetri & 15);
            let c;
            if fec < fecmax {
                let cf = vertexfifo[(vertex_off.wrapping_sub(1 + fec as usize)) & 15];
                c = if fec == 0 { next } else { cf };
                let fec0 = fec == 0;
                next += u32::from(fec0);
                push_vertex(&mut vertexfifo, &mut vertex_off, c, fec0);
            } else {
                if data > data_safe_end {
                    return Err(malformed());
                }
                c = if fec != 15 {
                    last.wrapping_add((fec * 2 - 27) as u32)
                } else {
                    decode_index(buffer, &mut data, last)
                };
                last = c;
                push_vertex(&mut vertexfifo, &mut vertex_off, c, true);
            }
            push_edge(&mut edgefifo, &mut edge_off, c, b);
            push_edge(&mut edgefifo, &mut edge_off, a, c);
            emit(out, a, b, c);
        } else if codetri < 0xfe {
            let codeaux = buffer[codeaux_table + usize::from(codetri & 15)];
            let feb = usize::from(codeaux >> 4);
            let fec = usize::from(codeaux & 15);
            let a = next;
            next += 1;
            let bf = vertexfifo[(vertex_off.wrapping_sub(feb)) & 15];
            let b = if feb == 0 { next } else { bf };
            next += u32::from(feb == 0);
            let cf = vertexfifo[(vertex_off.wrapping_sub(fec)) & 15];
            let c = if fec == 0 { next } else { cf };
            next += u32::from(fec == 0);
            emit(out, a, b, c);
            push_vertex(&mut vertexfifo, &mut vertex_off, a, true);
            push_vertex(&mut vertexfifo, &mut vertex_off, b, feb == 0);
            push_vertex(&mut vertexfifo, &mut vertex_off, c, fec == 0);
            push_edge(&mut edgefifo, &mut edge_off, b, a);
            push_edge(&mut edgefifo, &mut edge_off, c, b);
            push_edge(&mut edgefifo, &mut edge_off, a, c);
        } else {
            if data > data_safe_end {
                return Err(malformed());
            }
            let codeaux = buffer[data];
            data += 1;
            let fea = if codetri == 0xfe { 0 } else { 15 };
            let feb = usize::from(codeaux >> 4);
            let fec = usize::from(codeaux & 15);
            if codeaux == 0 {
                next = 0;
            }
            let mut a = if fea == 0 {
                next += 1;
                next - 1
            } else {
                0
            };
            let mut b = if feb == 0 {
                next += 1;
                next - 1
            } else {
                vertexfifo[(vertex_off.wrapping_sub(feb)) & 15]
            };
            let mut c = if fec == 0 {
                next += 1;
                next - 1
            } else {
                vertexfifo[(vertex_off.wrapping_sub(fec)) & 15]
            };
            if fea == 15 {
                a = decode_index(buffer, &mut data, last);
                last = a;
            }
            if feb == 15 {
                b = decode_index(buffer, &mut data, last);
                last = b;
            }
            if fec == 15 {
                c = decode_index(buffer, &mut data, last);
                last = c;
            }
            emit(out, a, b, c);
            push_vertex(&mut vertexfifo, &mut vertex_off, a, true);
            push_vertex(&mut vertexfifo, &mut vertex_off, b, feb == 0 || feb == 15);
            push_vertex(&mut vertexfifo, &mut vertex_off, c, fec == 0 || fec == 15);
            push_edge(&mut edgefifo, &mut edge_off, b, a);
            push_edge(&mut edgefifo, &mut edge_off, c, b);
            push_edge(&mut edgefifo, &mut edge_off, a, c);
        }
    }
    if data != data_safe_end {
        return Err(malformed());
    }
    Ok(())
}

/// `meshopt_decodeIndexSequence`: `count` indices of any topology.
pub fn decode_index_sequence(
    out: &mut [u8],
    count: usize,
    index_size: usize,
    buffer: &[u8],
) -> Result<(), String> {
    let malformed = || "meshopt: malformed index data".to_string();
    if !(index_size == 2 || index_size == 4) {
        return Err("meshopt: indices of 2 or 4 bytes".into());
    }
    if buffer.len() < 1 + count + 4 {
        return Err(malformed());
    }
    if buffer[0] & 0xf0 != SEQUENCE_HEADER || buffer[0] & 0x0f > 1 {
        return Err("meshopt: unsupported index encoding".into());
    }
    let data_safe_end = buffer.len() - 4;
    let mut data = 1;
    let mut last = [0u32; 2];
    for i in 0..count {
        if data >= data_safe_end {
            return Err(malformed());
        }
        let mut v = decode_vbyte(buffer, &mut data);
        let current = (v & 1) as usize;
        v >>= 1;
        let d = (v >> 1) ^ 0u32.wrapping_sub(v & 1);
        let index = last[current].wrapping_add(d);
        last[current] = index;
        write_index(out, i, index_size, index);
    }
    if data != data_safe_end {
        return Err(malformed());
    }
    Ok(())
}

// --- Filters (vertexfilter.cpp) -----------------------------------------------

// Rounded signed float → int, as the reference.
fn round_signed(v: f32) -> i32 {
    (v + if v >= 0.0 { 0.5 } else { -0.5 }) as i32
}

fn oct(x: f32, y: f32, z: f32, max: f32) -> (i32, i32, i32) {
    let z = z - x.abs() - y.abs();
    let t = if z >= 0.0 { 0.0 } else { z };
    let x = x + if x >= 0.0 { t } else { -t };
    let y = y + if y >= 0.0 { t } else { -t };
    let s = max / (x * x + y * y + z * z).sqrt();
    (
        round_signed(x * s),
        round_signed(y * s),
        round_signed(z * s),
    )
}

fn filter_oct8(data: &mut [u8]) {
    for e in data.chunks_exact_mut(4) {
        let (x, y, z) = oct(
            f32::from(e[0] as i8),
            f32::from(e[1] as i8),
            f32::from(e[2] as i8),
            127.0,
        );
        e[0] = x as i8 as u8;
        e[1] = y as i8 as u8;
        e[2] = z as i8 as u8;
    }
}

fn read16(e: &[u8], i: usize) -> i16 {
    i16::from_le_bytes([e[i * 2], e[i * 2 + 1]])
}

fn write16(e: &mut [u8], i: usize, v: i32) {
    e[i * 2..i * 2 + 2].copy_from_slice(&(v as i16).to_le_bytes());
}

fn filter_oct16(data: &mut [u8]) {
    for e in data.chunks_exact_mut(8) {
        let (x, y, z) = oct(
            f32::from(read16(e, 0)),
            f32::from(read16(e, 1)),
            f32::from(read16(e, 2)),
            32767.0,
        );
        write16(e, 0, x);
        write16(e, 1, y);
        write16(e, 2, z);
    }
}

fn filter_quat(data: &mut [u8]) {
    let scale = 32767.0 / 2f32.sqrt();
    for e in data.chunks_exact_mut(8) {
        let q3 = i32::from(read16(e, 3));
        let s = (q3 | 3) as f32;
        let x = f32::from(read16(e, 0));
        let y = f32::from(read16(e, 1));
        let z = f32::from(read16(e, 2));
        let ww = s * s * 2.0 - x * x - y * y - z * z;
        let w = ww.max(0.0).sqrt();
        let ss = scale / s;
        let qc = (q3 & 3) as usize;
        write16(e, (qc + 1) & 3, round_signed(x * ss));
        write16(e, (qc + 2) & 3, round_signed(y * ss));
        write16(e, (qc + 3) & 3, round_signed(z * ss));
        write16(e, qc, (w * ss + 0.5) as i32);
    }
}

fn filter_exp(data: &mut [u8]) {
    for e in data.chunks_exact_mut(4) {
        let v = u32::from_le_bytes([e[0], e[1], e[2], e[3]]);
        let m = ((v << 8) as i32) >> 8;
        let ex = (v as i32) >> 24;
        // ldexp(m, e) as the reference: 2^e built in the exponent bits.
        let f = f32::from_bits(((ex + 127) as u32) << 23) * m as f32;
        e.copy_from_slice(&f.to_bits().to_le_bytes());
    }
}

// YCoCg with the alpha's top bit marking the bit count (KHR_meshopt_compression).
fn color(y: i32, co: i32, cg: i32, a_raw: i32, max: f32) -> [i32; 4] {
    let mut r#as = a_raw;
    r#as |= r#as >> 1;
    r#as |= r#as >> 2;
    r#as |= r#as >> 4;
    r#as |= r#as >> 8;
    let r = y + co - cg;
    let g = y + cg;
    let b = y - co - cg;
    let a = ((a_raw << 1) & r#as) | (a_raw & 1);
    let ss = max / r#as as f32;
    [r, g, b, a].map(|c| (c as f32 * ss + 0.5) as i32)
}

fn filter_color8(data: &mut [u8]) {
    for e in data.chunks_exact_mut(4) {
        let c = color(
            i32::from(e[0]),
            i32::from(e[1] as i8),
            i32::from(e[2] as i8),
            i32::from(e[3]),
            255.0,
        );
        for (d, v) in e.iter_mut().zip(c) {
            *d = v as u8;
        }
    }
}

fn filter_color16(data: &mut [u8]) {
    for e in data.chunks_exact_mut(8) {
        let u = |i: usize| i32::from(u16::from_le_bytes([e[i * 2], e[i * 2 + 1]]));
        let c = color(
            u(0),
            i32::from(read16(e, 1)),
            i32::from(read16(e, 2)),
            u(3),
            65535.0,
        );
        for (i, v) in c.into_iter().enumerate() {
            e[i * 2..i * 2 + 2].copy_from_slice(&(v as u16).to_le_bytes());
        }
    }
}

/// Decodes every bufferView compressed with `EXT_meshopt_compression` (or
/// `KHR_meshopt_compression`) into its place in `buffers` — the parent
/// buffer, a fallback (`resolve_buffers` makes those zeroed).
pub fn decode_views(json: &serde_json::Value, buffers: &mut [Vec<u8>]) -> Result<(), String> {
    let views = json["bufferViews"]
        .as_array()
        .map_or(&[][..], Vec::as_slice);
    for (index, view) in views.iter().enumerate() {
        let exts = &view["extensions"];
        let Some(ext) = exts
            .get("EXT_meshopt_compression")
            .or_else(|| exts.get("KHR_meshopt_compression"))
        else {
            continue;
        };
        let get = |k: &str| ext.get(k).and_then(serde_json::Value::as_u64);
        let (Some(buffer), Some(length), Some(stride), Some(count)) = (
            get("buffer"),
            get("byteLength"),
            get("byteStride"),
            get("count"),
        ) else {
            return Err(format!(
                "meshopt: bufferView {} lacks its parameters",
                index
            ));
        };
        let offset = get("byteOffset").unwrap_or(0) as usize;
        let mode = match ext.get("mode").and_then(serde_json::Value::as_str) {
            Some("ATTRIBUTES") => Mode::Attributes,
            Some("TRIANGLES") => Mode::Triangles,
            Some("INDICES") => Mode::Indices,
            m => return Err(format!("meshopt: mode {m:?}")),
        };
        let filter = match ext.get("filter").and_then(serde_json::Value::as_str) {
            None | Some("NONE") => Filter::None,
            Some("OCTAHEDRAL") => Filter::Octahedral,
            Some("QUATERNION") => Filter::Quaternion,
            Some("EXPONENTIAL") => Filter::Exponential,
            Some("COLOR") => Filter::Color,
            Some(f) => return Err(format!("meshopt: filter {f}")),
        };
        let source = buffers
            .get(buffer as usize)
            .and_then(|b| b.get(offset..offset + length as usize))
            .ok_or_else(|| format!("meshopt: bufferView {} source out of range", index))?
            .to_vec();
        let start = view["byteOffset"].as_u64().unwrap_or(0) as usize;
        let len = view["byteLength"].as_u64().unwrap_or(0) as usize;
        let target = buffers
            .get_mut(view["buffer"].as_u64().unwrap_or(u64::MAX) as usize)
            .and_then(|b| b.get_mut(start..start + len))
            .ok_or_else(|| format!("meshopt: bufferView {} out of range", index))?;
        decode(
            target,
            count as usize,
            stride as usize,
            &source,
            mode,
            filter,
        )
        .map_err(|e| format!("{e} (bufferView {})", index))?;
    }
    Ok(())
}

// meshoptimizer's demo/tests.cpp vectors.
#[cfg(test)]
mod tests {
    use super::*;

    const INDEX_BUFFER: [u32; 12] = [0, 1, 2, 2, 1, 3, 4, 6, 5, 7, 8, 9];
    const INDEX_DATA_V0: [u8; 27] = [
        0xe0, 0xf0, 0x10, 0xfe, 0xff, 0xf0, 0x0c, 0xff, 0x02, 0x02, 0x02, 0x00, 0x76, 0x87, 0x56,
        0x67, 0x78, 0xa9, 0x86, 0x65, 0x89, 0x68, 0x98, 0x01, 0x69, 0x00, 0x00,
    ];
    const INDEX_BUFFER_TRICKY: [u32; 15] = [0, 1, 2, 2, 1, 3, 0, 1, 2, 2, 1, 5, 2, 1, 4];
    const INDEX_DATA_V1: [u8; 24] = [
        0xe1, 0xf0, 0x10, 0xfe, 0x1f, 0x3d, 0x00, 0x0a, 0x00, 0x76, 0x87, 0x56, 0x67, 0x78, 0xa9,
        0x86, 0x65, 0x89, 0x68, 0x98, 0x01, 0x69, 0x00, 0x00,
    ];
    const INDEX_SEQUENCE: [u32; 6] = [0, 1, 51, 2, 49, 1000];
    const INDEX_SEQUENCE_V1: [u8; 13] = [
        0xd1, 0x00, 0x04, 0xcd, 0x01, 0x04, 0x07, 0x98, 0x1f, 0x00, 0x00, 0x00, 0x00,
    ];
    // PV { px, py, pz: u16, nu, nv: u8, tx, ty: u16 } × 4.
    fn vertex_buffer() -> Vec<u8> {
        let rows: [[u16; 5]; 4] = [
            [0, 0, 0, 0, 0],
            [300, 0, 0, 500, 0],
            [0, 300, 0, 0, 500],
            [300, 300, 0, 500, 500],
        ];
        let mut out = Vec::new();
        for [px, py, pz, tx, ty] in rows {
            for v in [px, py, pz] {
                out.extend_from_slice(&v.to_le_bytes());
            }
            out.extend_from_slice(&[0, 0]);
            for v in [tx, ty] {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        out
    }
    const VERTEX_DATA_V0: [u8; 85] = [
        0xa0, 0x01, 0x3f, 0x00, 0x00, 0x00, 0x58, 0x57, 0x58, 0x01, 0x26, 0x00, 0x00, 0x00, 0x01,
        0x0c, 0x00, 0x00, 0x00, 0x58, 0x01, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01,
        0x3f, 0x00, 0x00, 0x00, 0x17, 0x18, 0x17, 0x01, 0x26, 0x00, 0x00, 0x00, 0x01, 0x0c, 0x00,
        0x00, 0x00, 0x17, 0x01, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    const VERTEX_DATA_V1: [u8; 44] = [
        0xa1, 0xee, 0xaa, 0xee, 0x00, 0x4b, 0x4b, 0x4b, 0x00, 0x00, 0x4b, 0x00, 0x00, 0x7d, 0x7d,
        0x7d, 0x00, 0x00, 0x7d, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x62, 0x00, 0x62,
    ];
    const VERTEX_DATA_V1_CUSTOM: [u8; 103] = [
        0xa1, 0xd4, 0x94, 0xd4, 0x01, 0x0e, 0x00, 0x58, 0x57, 0x58, 0x02, 0x02, 0x12, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x58, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
        0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x0e, 0x00, 0x7d, 0x7d, 0x7d, 0x01, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x7d, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x62,
    ];

    fn indices(bytes: &[u8]) -> Vec<u32> {
        bytes
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect()
    }

    #[test]
    fn index_buffers() {
        let mut out = vec![0u8; 12 * 4];
        decode_index_buffer(&mut out, 12, 4, &INDEX_DATA_V0).unwrap();
        assert_eq!(indices(&out), INDEX_BUFFER);
        let mut out = vec![0u8; 15 * 4];
        decode_index_buffer(&mut out, 15, 4, &INDEX_DATA_V1).unwrap();
        assert_eq!(indices(&out), INDEX_BUFFER_TRICKY);
        // 16-bit output.
        let mut out = vec![0u8; 12 * 2];
        decode_index_buffer(&mut out, 12, 2, &INDEX_DATA_V0).unwrap();
        let v: Vec<u32> = out
            .chunks_exact(2)
            .map(|c| u32::from(u16::from_le_bytes([c[0], c[1]])))
            .collect();
        assert_eq!(v, INDEX_BUFFER);
        // Extra bytes are rejected.
        let mut extra = INDEX_DATA_V0.to_vec();
        extra.insert(extra.len() - 16, 0);
        assert!(decode_index_buffer(&mut [0u8; 48], 12, 4, &extra).is_err());
    }

    #[test]
    fn index_sequence() {
        let mut out = vec![0u8; 6 * 4];
        decode_index_sequence(&mut out, 6, 4, &INDEX_SEQUENCE_V1).unwrap();
        assert_eq!(indices(&out), INDEX_SEQUENCE);
    }

    #[test]
    fn vertex_buffers() {
        for data in [
            &VERTEX_DATA_V0[..],
            &VERTEX_DATA_V1[..],
            &VERTEX_DATA_V1_CUSTOM[..],
        ] {
            let mut out = vec![0u8; 4 * 12];
            decode_vertex_buffer(&mut out, 4, 12, data).unwrap();
            assert_eq!(out, vertex_buffer());
        }
    }

    #[test]
    fn filters() {
        let mut oct8: Vec<u8> = vec![
            0, 1, 127, 0, 0, 187, 127, 1, 255, 1, 127, 0, 14, 130, 127, 1,
        ];
        filter_oct8(&mut oct8);
        assert_eq!(
            oct8,
            [0, 1, 127, 0, 0, 159, 82, 1, 255, 1, 127, 0, 1, 130, 241, 1]
        );

        let shorts = |v: &[u16]| v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>();
        let mut oct12 = shorts(&[
            0, 1, 2047, 0, 0, 1870, 2047, 1, 2017, 1, 2047, 0, 14, 1300, 2047, 1,
        ]);
        filter_oct16(&mut oct12);
        assert_eq!(
            oct12,
            shorts(&[
                0, 16, 32767, 0, 0, 32621, 3088, 1, 32764, 16, 471, 0, 307, 28541, 16093, 1
            ])
        );

        let mut quat = shorts(&[
            0, 1, 0, 0x7fc, 0, 1870, 0, 0x7fd, 2017, 1, 0, 0x7fe, 14, 1300, 0, 0x7ff,
        ]);
        filter_quat(&mut quat);
        assert_eq!(
            quat,
            shorts(&[
                32767, 0, 11, 0, 0, 25013, 0, 21166, 11, 0, 23504, 22830, 158, 14715, 0, 29277
            ])
        );

        let words = |v: &[u32]| v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>();
        let mut exp = words(&[0, 0xff00_0003, 0x02ff_fff7, 0xfe7f_ffff]);
        filter_exp(&mut exp);
        assert_eq!(exp, words(&[0, 0x3fc0_0000, 0xc210_0000, 0x49ff_fffe]));
    }
}
