//! Frame codes: a frame's identity drawn as black and white 16×16 luma blocks along the top
//! of the frame, readable after lossy encoding.

use sc_geometry::SizePx;

use crate::{CpuNv12Frame, Nv12View};

/// Side of one code block in pixels.
pub const CODE_BLOCK_PX: u32 = 16;
/// Bits in a code: marker byte, source hash, index, pts_ms, Fletcher-16 checksum.
pub const CODE_BITS: u32 = 120;

const MARKER: u8 = 0xB2;
const CODE_BYTES: usize = (CODE_BITS / 8) as usize;

/// Frame identity encoded in 16×16 luma blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FrameCode {
    pub source_hash: u32,
    pub index: u32,
    pub pts_ms: u32,
}

/// FNV-1a of the source name.
pub fn source_hash(name: &str) -> u32 {
    name.bytes().fold(0x811c_9dc5_u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    })
}

/// Blocks per row and block rows the code needs at this size, or `None` when it does not fit.
pub fn code_layout(size: SizePx) -> Option<(u32, u32)> {
    let per_row = size.w / CODE_BLOCK_PX;
    if per_row == 0 {
        return None;
    }
    let rows = CODE_BITS.div_ceil(per_row);
    (rows * CODE_BLOCK_PX <= size.h).then_some((per_row, rows))
}

fn fletcher16(bytes: &[u8]) -> u16 {
    let (mut a, mut b) = (0u16, 0u16);
    for &x in bytes {
        a = (a + u16::from(x)) % 255;
        b = (b + a) % 255;
    }
    (b << 8) | a
}

fn to_bytes(c: FrameCode) -> [u8; CODE_BYTES] {
    let mut out = [0u8; CODE_BYTES];
    out[0] = MARKER;
    out[1..5].copy_from_slice(&c.source_hash.to_be_bytes());
    out[5..9].copy_from_slice(&c.index.to_be_bytes());
    out[9..13].copy_from_slice(&c.pts_ms.to_be_bytes());
    let sum = fletcher16(&out[..13]);
    out[13..15].copy_from_slice(&sum.to_be_bytes());
    out
}

fn levels(full_range: bool) -> (u8, u8) {
    if full_range { (0, 255) } else { (16, 235) }
}

/// Draws `code` into the top rows of `frame` (neutral chroma under it). Returns false when the
/// frame is too small (see [`code_layout`]) or its planes are short.
pub fn write_code(frame: &mut CpuNv12Frame, code: FrameCode) -> bool {
    let Some((per_row, rows)) = code_layout(frame.size) else {
        return false;
    };
    let w = frame.size.w as usize;
    let uv_stride = frame.uv_stride();
    if frame.y.len() < w * frame.size.h as usize || frame.uv.len() < uv_stride * frame.uv_rows() {
        return false;
    }
    let (lo, hi) = levels(frame.full_range);
    let bytes = to_bytes(code);
    let block = CODE_BLOCK_PX as usize;
    for bit in 0..CODE_BITS as usize {
        let on = (bytes[bit / 8] >> (7 - bit % 8)) & 1 == 1;
        let bx = (bit % per_row as usize) * block;
        let by = (bit / per_row as usize) * block;
        for y in by..by + block {
            frame.y[y * w + bx..y * w + bx + block].fill(if on { hi } else { lo });
        }
    }
    let code_w = per_row as usize * block;
    for row in 0..rows as usize * block / 2 {
        frame.uv[row * uv_stride..row * uv_stride + code_w].fill(128);
    }
    true
}

/// Reads a code drawn by [`write_code`]: the mean of each block's inner 8×8 luma against the
/// mid-grey threshold, then the marker and checksum. `None` when absent or damaged.
pub fn read_code(v: &Nv12View) -> Option<FrameCode> {
    let (per_row, rows) = code_layout(v.size)?;
    let block = CODE_BLOCK_PX as usize;
    let needed_rows = rows as usize * block;
    let w = v.size.w as usize;
    if v.y_stride < w || v.y.len() < (needed_rows - 1) * v.y_stride + w {
        return None;
    }
    let (lo, hi) = levels(v.full_range);
    let threshold = (u32::from(lo) + u32::from(hi)) / 2;
    let mut bytes = [0u8; CODE_BYTES];
    for bit in 0..CODE_BITS as usize {
        let bx = (bit % per_row as usize) * block;
        let by = (bit / per_row as usize) * block;
        let mut sum = 0u32;
        for y in by + 4..by + 12 {
            let row = &v.y[y * v.y_stride + bx + 4..y * v.y_stride + bx + 12];
            sum += row.iter().map(|&p| u32::from(p)).sum::<u32>();
        }
        if sum / 64 >= threshold {
            bytes[bit / 8] |= 1 << (7 - bit % 8);
        }
    }
    if bytes[0] != MARKER || fletcher16(&bytes[..13]).to_be_bytes() != [bytes[13], bytes[14]] {
        return None;
    }
    let word = |i: usize| u32::from_be_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
    Some(FrameCode {
        source_hash: word(1),
        index: word(5),
        pts_ms: word(9),
    })
}

/// A flat frame whose colour depends on `source_hash`, so different sources look different.
pub fn background(size: SizePx, source_hash: u32, full_range: bool) -> CpuNv12Frame {
    let y = 48 + (source_hash % 128) as u8;
    let u = 64 + ((source_hash >> 8) % 128) as u8;
    let v = 64 + ((source_hash >> 16) % 128) as u8;
    CpuNv12Frame::filled(size, y, u, v, full_range)
}

/// [`background`] for `code.source_hash` with `code` drawn on it (when it fits).
pub fn coded_frame(size: SizePx, code: FrameCode, full_range: bool) -> CpuNv12Frame {
    let mut f = background(size, code.source_hash, full_range);
    write_code(&mut f, code);
    f
}

/// `pts_ms` of a code: host nanoseconds / 1e6, wrapped to 32 bits.
pub fn pts_ms(host_ns: i64) -> u32 {
    (host_ns.div_euclid(1_000_000) as u64 & 0xffff_ffff) as u32
}
