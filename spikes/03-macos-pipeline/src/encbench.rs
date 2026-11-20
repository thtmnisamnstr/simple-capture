use std::ptr::NonNull;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use objc2_av_foundation::AVAssetWriterInputPixelBufferAdaptor;
use objc2_core_foundation::CFRetained;
use objc2_core_video::*;
use serde_json::json;

use crate::capture::PIX_420V;
use crate::util::*;
use crate::writer::{TrackWriter, VideoCfg};
use crate::Args;

fn texel(x: usize, y: usize) -> u8 {
    let gx = x / 2;
    let gy = y / 2;
    let line = (gy / 9) % 2 == 0;
    let mut hsh = (gx as u32).wrapping_mul(0x9E37_79B1) ^ ((gy / 9) as u32).wrapping_mul(0x85EB_CA77);
    hsh ^= hsh >> 15;
    let glyph = ((gx / 6) as u32).wrapping_mul(2654435761) >> 28;
    if line && glyph > 3 && (hsh >> 7) & 3 != 0 && (gy % 9) < 7 { 40 } else { 235 }
}

fn fill_scroll(pb: &CVPixelBuffer, k: usize) {
    unsafe {
        CVPixelBufferLockBaseAddress(pb, CVPixelBufferLockFlags(0));
        let w = CVPixelBufferGetWidthOfPlane(pb, 0);
        let h = CVPixelBufferGetHeightOfPlane(pb, 0);
        let ys = CVPixelBufferGetBytesPerRowOfPlane(pb, 0);
        let y = CVPixelBufferGetBaseAddressOfPlane(pb, 0) as *mut u8;
        let (bx0, bx1, by0, by1) = (w / 10, w * 6 / 10, h / 5, h * 4 / 5);
        let (mx, my, ms) = ((k * 40) % (w / 2) + w / 2, h / 3, h / 6);
        for row in 0..h {
            let line = std::slice::from_raw_parts_mut(y.add(row * ys), w);
            for (x, px) in line.iter_mut().enumerate() {
                *px = if x >= mx && x < mx + ms && row >= my && row < my + ms {
                    120
                } else if x >= bx0 && x < bx1 && row >= by0 && row < by1 {
                    texel(x, row + k * 12)
                } else {
                    texel(x, row)
                };
            }
        }
        let cw = CVPixelBufferGetWidthOfPlane(pb, 1);
        let ch = CVPixelBufferGetHeightOfPlane(pb, 1);
        let cs = CVPixelBufferGetBytesPerRowOfPlane(pb, 1);
        let c = CVPixelBufferGetBaseAddressOfPlane(pb, 1) as *mut u8;
        for row in 0..ch {
            let line = std::slice::from_raw_parts_mut(c.add(row * cs), cw * 2);
            for x in 0..cw {
                let inbox = 2 * x >= mx && 2 * x < mx + ms && 2 * row >= my && 2 * row < my + ms;
                line[2 * x] = if inbox { 200 } else { 128 };
                line[2 * x + 1] = if inbox { 60 } else { 128 };
            }
        }
        CVPixelBufferUnlockBaseAddress(pb, CVPixelBufferLockFlags(0));
    }
}

fn fill_noise(pb: &CVPixelBuffer, k: usize) {
    unsafe {
        CVPixelBufferLockBaseAddress(pb, CVPixelBufferLockFlags(0));
        let w = CVPixelBufferGetWidthOfPlane(pb, 0);
        let h = CVPixelBufferGetHeightOfPlane(pb, 0);
        let ys = CVPixelBufferGetBytesPerRowOfPlane(pb, 0);
        let y = CVPixelBufferGetBaseAddressOfPlane(pb, 0) as *mut u8;
        let mut seed = 0x9E37_79B9u32.wrapping_mul(k as u32 + 1);
        for row in 0..h {
            let line = std::slice::from_raw_parts_mut(y.add(row * ys), w);
            let text_band = (row / 24) % 3 == 0;
            for (x, px) in line.iter_mut().enumerate() {
                *px = if text_band && ((x + k * 13) / 8) % 4 != 0 {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    if seed & 1 == 0 { 30 } else { 220 }
                } else {
                    (16 + ((x + row + k * 24) % 200)) as u8
                };
            }
        }
        let cw = CVPixelBufferGetWidthOfPlane(pb, 1);
        let ch = CVPixelBufferGetHeightOfPlane(pb, 1);
        let cs = CVPixelBufferGetBytesPerRowOfPlane(pb, 1);
        let c = CVPixelBufferGetBaseAddressOfPlane(pb, 1) as *mut u8;
        for row in 0..ch {
            let line = std::slice::from_raw_parts_mut(c.add(row * cs), cw * 2);
            for x in 0..cw {
                line[2 * x] = (64 + ((x + k * 8) % 128)) as u8;
                line[2 * x + 1] = (64 + ((row + k * 4) % 128)) as u8;
            }
        }
        CVPixelBufferUnlockBaseAddress(pb, CVPixelBufferLockFlags(0));
    }
}

fn run_one(dir: &std::path::Path, w: usize, h: usize, fps: f64, frames: usize, hevc: bool, bpp: f64, noise: bool, realtime: bool) -> Result<serde_json::Value> {
    let path = dir.join(format!("encbench_{}x{}_{}.mov", w, h, if hevc { "hevc" } else { "h264" }));
    let cfg = VideoCfg { width: w, height: h, fps, bitrate: (w * h) as f64 * fps * bpp, hevc, realtime };
    let mut tw = TrackWriter::video("encbench", &path, &cfg, 1.0)?;
    let (writer, input) = tw.raw();
    let pf = num_i(PIX_420V as i64);
    let wv = num_i(w as i64);
    let hv = num_i(h as i64);
    let empty: objc2::rc::Retained<objc2_foundation::NSDictionary<objc2_foundation::NSString, objc2::runtime::AnyObject>> = objc2_foundation::NSDictionary::new();
    let attrs = dict(&[
        (cfstr_as_ns(unsafe { kCVPixelBufferPixelFormatTypeKey }), as_obj(&*pf)),
        (cfstr_as_ns(unsafe { kCVPixelBufferWidthKey }), as_obj(&*wv)),
        (cfstr_as_ns(unsafe { kCVPixelBufferHeightKey }), as_obj(&*hv)),
        (cfstr_as_ns(unsafe { kCVPixelBufferIOSurfacePropertiesKey }), as_obj(&*empty)),
    ]);
    let adaptor = unsafe { AVAssetWriterInputPixelBufferAdaptor::assetWriterInputPixelBufferAdaptorWithAssetWriterInput_sourcePixelBufferAttributes(input, Some(&attrs)) };
    unsafe {
        if !writer.startWriting() {
            return Err(anyhow!("startWriting failed {}x{}: {:?}", w, h, writer.error().map(|e| e.localizedDescription())));
        }
        writer.startSessionAtSourceTime(cmtime_secs(0.0));
    }
    let pool = unsafe { adaptor.pixelBufferPool() }.ok_or_else(|| anyhow!("no pool"))?;
    let k = 16;
    let mut bufs: Vec<CFRetained<CVPixelBuffer>> = vec![];
    for i in 0..k {
        let mut out: *mut CVPixelBuffer = std::ptr::null_mut();
        let r = unsafe { CVPixelBufferPool::create_pixel_buffer(None, &pool, NonNull::from(&mut out)) };
        if r != 0 {
            return Err(anyhow!("pool alloc {r}"));
        }
        let pb = unsafe { CFRetained::from_raw(NonNull::new(out).unwrap()) };
        if noise { fill_noise(&pb, i) } else { fill_scroll(&pb, i) }
        bufs.push(pb);
    }
    tw.mark_started(0.0);
    let (writer, input) = tw.raw();
    let t = Instant::now();
    let mut waits = 0u64;
    for i in 0..frames {
        while !unsafe { input.isReadyForMoreMediaData() } {
            waits += 1;
            std::thread::sleep(Duration::from_micros(100));
        }
        let ok = unsafe { adaptor.appendPixelBuffer_withPresentationTime(&bufs[i % k], cmtime_secs(i as f64 / fps)) };
        if !ok {
            return Err(anyhow!("append failed at {i}: {:?}", unsafe { writer.error() }.map(|e| e.localizedDescription())));
        }
    }
    drop(bufs);
    tw.finish(Some(cmtime_secs(frames as f64 / fps)))?;
    let dt = t.elapsed().as_secs_f64();
    let enc_fps = frames as f64 / dt;
    let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let _ = std::fs::remove_file(&path);
    Ok(json!({
        "realtime_mode": realtime, "content": if noise {"noise"} else {"scroll"}, "size": [w, h], "codec": if hevc {"hevc"} else {"h264"}, "frames": frames, "seconds": dt, "encode_fps": enc_fps,
        "headroom_vs_30fps": enc_fps / 30.0, "headroom_vs_60fps": enc_fps / 60.0, "ready_waits": waits,
        "target_mbps": cfg.bitrate / 1e6, "output_mbps": bytes as f64 * 8.0 / (frames as f64 / fps) / 1e6,
    }))
}

pub fn encbench(a: &Args) -> Result<()> {
    let dir = std::path::PathBuf::from(a.get("out").unwrap_or("out/encbench"));
    std::fs::create_dir_all(&dir)?;
    let frames = a.num("frames", 300.0) as usize;
    let bpp = a.num("bpp", 0.05);
    let sizes: Vec<(usize, usize)> = a
        .get("sizes")
        .unwrap_or("7680x2160,5120x2880,3840x2160,3024x1964,1920x1080")
        .split(',')
        .filter_map(|s| s.split_once('x').and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?))))
        .collect();
    let mut res = vec![];
    for &(w, h) in &sizes {
        for hevc in [true, false] {
            if !hevc && a.flag("hevc-only") {
                continue;
            }
            match run_one(&dir, w, h, 60.0, frames, hevc, bpp, a.get("content") == Some("noise"), a.get("realtime") != Some("false")) {
                Ok(v) => {
                    eprintln!("{v}");
                    res.push(v)
                }
                Err(e) => {
                    eprintln!("{w}x{h} hevc={hevc}: {e}");
                    res.push(json!({"size": [w, h], "codec": if hevc {"hevc"} else {"h264"}, "error": e.to_string()}))
                }
            }
        }
    }
    let out = json!({"note": "AVAssetWriter (expectsMediaDataInRealTime per realtime_mode; default true like the recorder) fed IOSurface-backed 420v buffers as fast as it accepts them; 16 synthetic frames cycled; content=scroll: static text-like page with a 50%x60% region scrolling 12 px/frame plus a moving box (realistic heavy screen motion); content=noise: per-frame random text bands (adversarial); timestamps at 60 fps", "results": res});
    write_json(&dir.join("encbench.json"), &out)?;
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}
