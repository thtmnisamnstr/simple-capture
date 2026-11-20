use std::f32::consts::PI;
use std::io::Write;
use std::path::Path;

pub const RATE: u32 = 48_000;

pub fn write_beep_wav(path: &Path, seconds: u32) -> std::io::Result<()> {
    let channels = 2u16;
    let frames = RATE * seconds;
    let beep = RATE / 10;
    let fade = RATE / 200;
    let mut data = Vec::with_capacity(frames as usize * 4);
    for i in 0..frames {
        let pos = i % RATE;
        let s = if pos < beep {
            let env = (pos.min(beep - 1 - pos) as f32 / fade as f32).min(1.0);
            0.5 * env * (2.0 * PI * 1000.0 * i as f32 / RATE as f32).sin()
        } else {
            0.0
        };
        let v = (s * i16::MAX as f32) as i16;
        for _ in 0..channels {
            data.extend_from_slice(&v.to_le_bytes());
        }
    }
    let mut f = std::fs::File::create(path)?;
    let byte_rate = RATE * channels as u32 * 2;
    f.write_all(b"RIFF")?;
    f.write_all(&(36 + data.len() as u32).to_le_bytes())?;
    f.write_all(b"WAVEfmt ")?;
    f.write_all(&16u32.to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?;
    f.write_all(&channels.to_le_bytes())?;
    f.write_all(&RATE.to_le_bytes())?;
    f.write_all(&byte_rate.to_le_bytes())?;
    f.write_all(&(channels * 2).to_le_bytes())?;
    f.write_all(&16u16.to_le_bytes())?;
    f.write_all(b"data")?;
    f.write_all(&(data.len() as u32).to_le_bytes())?;
    f.write_all(&data)?;
    Ok(())
}
