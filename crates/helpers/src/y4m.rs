use sc_geometry::SizePx;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Y4mHeader {
    pub size: SizePx,
    pub fps_num: u32,
    pub fps_den: u32,
}

/// Stream header line for 4:2:0 BT.709 limited-range frames, as verified in spike 1.
pub fn header(h: &Y4mHeader) -> String {
    format!(
        "YUV4MPEG2 W{} H{} F{}:{} Ip A1:1 C420mpeg2 XYSCSS=420MPEG2 XCOLORRANGE=LIMITED\n",
        h.size.w, h.size.h, h.fps_num, h.fps_den
    )
}

pub const FRAME: &[u8] = b"FRAME\n";

/// Bytes of one I420 frame payload (after [`FRAME`]); odd sizes round the chroma planes up.
pub fn i420_frame_bytes(size: SizePx) -> usize {
    let (w, h) = (size.w as usize, size.h as usize);
    w * h + 2 * w.div_ceil(2) * h.div_ceil(2)
}
