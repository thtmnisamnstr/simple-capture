use sc_geometry::SizePx;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Y4mHeader {
    pub size: SizePx,
    pub fps_num: u32,
    pub fps_den: u32,
}

/// Stream header line. Stub until wp-helpers.
pub fn header(_h: &Y4mHeader) -> String {
    String::new()
}

pub const FRAME: &[u8] = b"FRAME\n";
