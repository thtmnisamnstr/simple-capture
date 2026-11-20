use std::path::Path;
use std::ptr::NonNull;
use std::time::Instant;

use anyhow::{anyhow, Result};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::AnyThread;
use objc2_av_foundation::*;
use objc2_core_foundation::CFRetained;
use objc2_core_media::{CMSampleBuffer, CMTime, CMTimeRange};
use objc2_core_video::*;
use objc2_foundation::{NSDictionary, NSString, NSURL};
use objc2_metal::{MTLDevice, MTLPixelFormat, MTLTexture, MTLTextureType};
use serde_json::json;

use crate::capture::PIX_420V;
use crate::record::summarize;
use crate::util::*;
use crate::Args;

pub struct Reader {
    pub reader: Retained<AVAssetReader>,
    pub output: Retained<AVAssetReaderTrackOutput>,
    pub size: (f64, f64),
    pub duration: f64,
}

pub fn open_reader(path: &Path, range: Option<(f64, f64)>, pixfmt: u32) -> Result<Reader> {
    unsafe {
        let url = NSURL::fileURLWithPath(&NSString::from_str(path.to_str().unwrap()));
        let asset = AVURLAsset::URLAssetWithURL_options(&url, None);
        #[allow(deprecated)]
        let tracks = asset.tracksWithMediaType(AVMediaTypeVideo.unwrap());
        let track = tracks.firstObject().ok_or_else(|| anyhow!("no video track in {}", path.display()))?;
        let sz = track.naturalSize();
        let tr = track.timeRange();
        let reader = AVAssetReader::initWithAsset_error(AVAssetReader::alloc(), &asset).map_err(|e| anyhow!("AVAssetReader: {}", e.localizedDescription()))?;
        let pf = num_i(pixfmt as i64);
        let yes = num_b(true);
        let empty: Retained<NSDictionary<NSString, objc2::runtime::AnyObject>> = NSDictionary::new();
        let settings = dict(&[
            (cfstr_as_ns(kCVPixelBufferPixelFormatTypeKey), as_obj(&*pf)),
            (cfstr_as_ns(kCVPixelBufferMetalCompatibilityKey), as_obj(&*yes)),
            (cfstr_as_ns(kCVPixelBufferIOSurfacePropertiesKey), as_obj(&*empty)),
        ]);
        let output = AVAssetReaderTrackOutput::initWithTrack_outputSettings(AVAssetReaderTrackOutput::alloc(), &track, Some(&settings));
        output.setAlwaysCopiesSampleData(false);
        reader.addOutput(&output);
        if let Some((s, d)) = range {
            reader.setTimeRange(CMTimeRange { start: CMTime::with_seconds(s, 600), duration: CMTime::with_seconds(d, 600) });
        }
        if !reader.startReading() {
            return Err(anyhow!("startReading: {:?}", reader.error().map(|e| e.localizedDescription())));
        }
        Ok(Reader { reader, output, size: (sz.width, sz.height), duration: tr.duration.seconds() })
    }
}

impl Reader {
    pub fn next(&self) -> Option<Retained<CMSampleBuffer>> {
        unsafe { self.output.copyNextSampleBuffer() }
    }
}

struct KeepAlive(#[allow(dead_code)] CFRetained<CVMetalTexture>);
unsafe impl Send for KeepAlive {}
unsafe impl Sync for KeepAlive {}

pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub adapter_name: String,
    cache: CFRetained<CVMetalTextureCache>,
    pipeline: wgpu::RenderPipeline,
    bgl: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

pub struct Imported {
    pub y: wgpu::Texture,
    pub uv: wgpu::Texture,
    pub proof: serde_json::Value,
}

const SHADER: &str = r#"
@group(0) @binding(0) var y_tex: texture_2d<f32>;
@group(0) @binding(1) var uv_tex: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

struct VOut { @builtin(position) pos: vec4f, @location(0) uv: vec2f };

@vertex fn vs(@builtin(vertex_index) i: u32) -> VOut {
    let p = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    var o: VOut;
    o.pos = vec4f(p * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2f(p.x, 1.0 - p.y);
    return o;
}

@fragment fn fs(v: VOut) -> @location(0) vec4f {
    let y = textureSample(y_tex, samp, v.uv).r;
    let c = textureSample(uv_tex, samp, v.uv).rg;
    let yy = (y - 16.0 / 255.0) * (255.0 / 219.0);
    let cb = (c.x - 128.0 / 255.0) * (255.0 / 224.0);
    let cr = (c.y - 128.0 / 255.0) * (255.0 / 224.0);
    let r = yy + 1.5748 * cr;
    let g = yy - 0.187324 * cb - 0.468124 * cr;
    let b = yy + 1.8556 * cb;
    return vec4f(clamp(vec3f(r, g, b), vec3f(0.0), vec3f(1.0)), 1.0);
}
"#;

impl Gpu {
    pub fn new() -> Result<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor { backends: wgpu::Backends::METAL, ..wgpu::InstanceDescriptor::new_without_display_handle() });
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))?;
        let adapter_name = adapter.get_info().name;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))?;
        let mtl: Retained<ProtocolObject<dyn MTLDevice>> = unsafe {
            let hal = device.as_hal::<wgpu_hal::api::Metal>().ok_or_else(|| anyhow!("not a Metal device"))?;
            hal.raw_device().clone()
        };
        let mut cache_out: *mut CVMetalTextureCache = std::ptr::null_mut();
        let r = unsafe { CVMetalTextureCache::create(None, None, &mtl, None, NonNull::from(&mut cache_out)) };
        if r != 0 || cache_out.is_null() {
            return Err(anyhow!("CVMetalTextureCacheCreate failed: {r}"));
        }
        let cache = unsafe { CFRetained::from_raw(NonNull::new(cache_out).unwrap()) };
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("nv12"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let tex_entry = |b| wgpu::BindGroupLayoutEntry {
            binding: b,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
            count: None,
        };
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                tex_entry(0),
                tex_entry(1),
                wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&bgl)], ..Default::default() });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("nv12->rgba"),
            layout: Some(&pl),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba8Unorm, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor { mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
        Ok(Self { device, queue, adapter_name, cache, pipeline, bgl, sampler })
    }

    fn import_plane(&self, pb: &CVPixelBuffer, plane: usize, mfmt: MTLPixelFormat, wfmt: wgpu::TextureFormat) -> Result<(wgpu::Texture, serde_json::Value)> {
        let w = CVPixelBufferGetWidthOfPlane(pb, plane);
        let h = CVPixelBufferGetHeightOfPlane(pb, plane);
        let mut out: *mut CVMetalTexture = std::ptr::null_mut();
        let r = unsafe { CVMetalTextureCache::create_texture_from_image(None, &self.cache, pb, None, mfmt, w, h, plane, NonNull::from(&mut out)) };
        if r != 0 || out.is_null() {
            return Err(anyhow!("CVMetalTextureCacheCreateTextureFromImage plane {plane}: {r}"));
        }
        let cvtex = unsafe { CFRetained::from_raw(NonNull::new(out).unwrap()) };
        let raw = CVMetalTextureGetTexture(&cvtex);
        let mtl: Retained<ProtocolObject<dyn MTLTexture>> = unsafe { raw.ok_or_else(|| anyhow!("CVMetalTexture has no MTLTexture"))? };
        let mtl_ptr = Retained::as_ptr(&mtl) as usize;
        let tex_surface_id = mtl.iosurface().map(|s| s.id());
        let tex_plane = mtl.iosurfacePlane();
        let keep = KeepAlive(cvtex);
        let hal = unsafe {
            wgpu_hal::metal::Device::texture_from_raw(
                mtl,
                wfmt,
                MTLTextureType::Type2D,
                1,
                1,
                wgpu_hal::CopyExtent { width: w as u32, height: h as u32, depth: 1 },
                Some(Box::new(move || drop(keep))),
            )
        };
        let tex = unsafe {
            self.device.create_texture_from_hal::<wgpu_hal::api::Metal>(
                hal,
                &wgpu::TextureDescriptor {
                    label: Some("nv12-plane"),
                    size: wgpu::Extent3d { width: w as u32, height: h as u32, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wfmt,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
                wgpu::TextureUses::RESOURCE,
            )
        };
        let back_ptr = unsafe { tex.as_hal::<wgpu_hal::api::Metal>().map(|t| t.raw_handle() as *const ProtocolObject<dyn MTLTexture> as *const u8 as usize) };
        let proof = json!({
            "plane": plane, "size": [w, h], "format": format!("{wfmt:?}"),
            "mtl_texture_iosurface_id": tex_surface_id, "mtl_texture_iosurface_plane": tex_plane,
            "wgpu_texture_raw_handle_is_same_mtl_object": back_ptr.map(|p| p == mtl_ptr),
        });
        Ok((tex, proof))
    }

    pub fn import_nv12(&self, sb: &CMSampleBuffer) -> Result<Imported> {
        let ib = unsafe { sb.image_buffer() }.ok_or_else(|| anyhow!("no image buffer"))?;
        let pb: &CVPixelBuffer = &ib;
        let fmt = CVPixelBufferGetPixelFormatType(pb);
        if fmt != PIX_420V {
            return Err(anyhow!("expected 420v, got {}", fourcc(fmt)));
        }
        let surf = CVPixelBufferGetIOSurface(Some(pb));
        let surf_id = surf.as_ref().map(|s| s.id());
        let (y, py) = self.import_plane(pb, 0, MTLPixelFormat::R8Unorm, wgpu::TextureFormat::R8Unorm)?;
        let (uv, puv) = self.import_plane(pb, 1, MTLPixelFormat::RG8Unorm, wgpu::TextureFormat::Rg8Unorm)?;
        let same = |p: &serde_json::Value| p["mtl_texture_iosurface_id"].as_u64() == surf_id.map(|v| v as u64);
        let proof = json!({
            "pixel_buffer_format": fourcc(fmt),
            "pixel_buffer_iosurface_id": surf_id,
            "planes": [py.clone(), puv.clone()],
            "textures_alias_pixel_buffer_iosurface": same(&py) && same(&puv),
            "cpu_lock_base_address_called": false,
        });
        Ok(Imported { y, uv, proof })
    }

    pub fn render(&self, imp: &Imported, target: &wgpu::Texture) {
        let vy = imp.y.create_view(&Default::default());
        let vuv = imp.uv.create_view(&Default::default());
        let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&vy) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&vuv) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });
        let tv = target.create_view(&Default::default());
        let mut enc = self.device.create_command_encoder(&Default::default());
        {
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &tv,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                ..Default::default()
            });
            rp.set_pipeline(&self.pipeline);
            rp.set_bind_group(0, &bg, &[]);
            rp.draw(0..3, 0..1);
        }
        self.queue.submit([enc.finish()]);
    }

    pub fn target(&self, w: u32, h: u32) -> wgpu::Texture {
        self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("target"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        })
    }

    pub fn wait(&self) {
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
    }

    pub fn readback(&self, target: &wgpu::Texture) -> Vec<u8> {
        let (w, h) = (target.width(), target.height());
        let row = (w * 4).div_ceil(256) * 256;
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: (row * h) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        let mut enc = self.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            target.as_image_copy(),
            wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) } },
            target.size(),
        );
        self.queue.submit([enc.finish()]);
        buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        self.wait();
        let data = buf.slice(..).get_mapped_range().expect("map");
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            let s = (y * row) as usize;
            out.extend_from_slice(&data[s..s + (w * 4) as usize]);
        }
        out
    }
}

pub fn save_png(path: &Path, w: u32, h: u32, rgba: &[u8]) -> Result<()> {
    let f = std::fs::File::create(path)?;
    let mut e = png::Encoder::new(std::io::BufWriter::new(f), w, h);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
    e.write_header()?.write_image_data(rgba)?;
    Ok(())
}

fn region_mean(rgba: &[u8], w: u32, rect: [f64; 4]) -> [f64; 3] {
    let (x0, y0) = ((rect[0] + rect[2] * 0.15) as u32, (rect[1] + rect[3] * 0.15) as u32);
    let (x1, y1) = ((rect[0] + rect[2] * 0.85) as u32, (rect[1] + rect[3] * 0.85) as u32);
    let mut acc = [0f64; 3];
    let mut n = 0f64;
    for y in (y0..y1).step_by(4) {
        for x in (x0..x1).step_by(4) {
            let i = ((y * w + x) * 4) as usize;
            if i + 2 < rgba.len() {
                for c in 0..3 {
                    acc[c] += rgba[i + c] as f64;
                }
                n += 1.0;
            }
        }
    }
    acc.map(|v| if n > 0.0 { v / n } else { 0.0 })
}

pub fn decode(a: &Args) -> Result<()> {
    let dir = std::path::PathBuf::from(a.get("dir").ok_or_else(|| anyhow!("--dir PROJECT"))?);
    let file = dir.join(a.get("file").unwrap_or("screen.mov"));
    let frame_at: f64 = a.num("frame-at", -1.0);
    let seeks = a.num("seeks", 20.0) as usize;
    let gpu = Gpu::new()?;
    let mut report = json!({"file": file.to_string_lossy(), "adapter": gpu.adapter_name});

    let t = Instant::now();
    let r = open_reader(&file, None, PIX_420V)?;
    let open_ms = t.elapsed().as_secs_f64() * 1e3;
    let (w, h) = (r.size.0 as u32, r.size.1 as u32);
    let mut n = 0u64;
    let mut first_ms = None;
    let mut pts = vec![];
    while let Some(sb) = r.next() {
        if unsafe { sb.image_buffer() }.is_none() {
            continue;
        }
        if first_ms.is_none() {
            first_ms = Some(t.elapsed().as_secs_f64() * 1e3);
        }
        pts.push(secs(unsafe { sb.presentation_time_stamp() }));
        n += 1;
    }
    let dt = t.elapsed().as_secs_f64();
    report["decode_only"] = json!({"frames": n, "seconds": dt, "fps": n as f64 / dt, "open_ms": open_ms, "first_frame_ms": first_ms, "media_duration_s": r.duration, "size": [w, h]});

    let pw = 1920u32.min(w);
    let ph = ((pw as f64) * (h as f64) / (w as f64)).round() as u32 & !1;
    let preview = gpu.target(pw, ph);
    let r = open_reader(&file, None, PIX_420V)?;
    let t = Instant::now();
    let mut n2 = 0u64;
    let mut import_us = vec![];
    while let Some(sb) = r.next() {
        if unsafe { sb.image_buffer() }.is_none() {
            continue;
        }
        let ti = Instant::now();
        let imp = gpu.import_nv12(&sb)?;
        import_us.push(ti.elapsed().as_secs_f64() * 1e6);
        gpu.render(&imp, &preview);
        gpu.wait();
        n2 += 1;
    }
    let dt2 = t.elapsed().as_secs_f64();
    report["decode_import_render_preview"] = json!({"frames": n2, "seconds": dt2, "fps": n2 as f64 / dt2, "preview_size": [pw, ph], "import_us": summarize(&import_us), "note": "each frame: AVAssetReader decode -> CVMetalTextureCache -> wgpu create_texture_from_hal -> NV12->RGB render -> wait idle"});

    let target_t = if frame_at >= 0.0 { frame_at } else { pts.get(pts.len() / 2).copied().unwrap_or(0.0) - pts.first().copied().unwrap_or(0.0) };
    let r = open_reader(&file, None, PIX_420V)?;
    let mut idx = 0usize;
    let base = pts.first().copied().unwrap_or(0.0);
    let mut chosen = None;
    while let Some(sb) = r.next() {
        if unsafe { sb.image_buffer() }.is_none() {
            continue;
        }
        let p = secs(unsafe { sb.presentation_time_stamp() }) - base;
        if p + 1e-6 >= target_t {
            chosen = Some((idx, p, sb));
            break;
        }
        idx += 1;
    }
    let (idx, p, sb) = chosen.ok_or_else(|| anyhow!("no frame at {target_t}"))?;
    let imp = gpu.import_nv12(&sb)?;
    let full = gpu.target(w, h);
    gpu.render(&imp, &full);
    let rgba = gpu.readback(&full);
    let png_path = dir.join(format!("{}.frame.png", file.file_stem().unwrap().to_string_lossy()));
    save_png(&png_path, w, h, &rgba)?;
    report["png"] = json!({"path": png_path.to_string_lossy(), "frame_index": idx, "frame_time_s": p});
    report["zero_copy_proof"] = imp.proof.clone();
    drop(imp);

    if let Ok(pj) = std::fs::read_to_string(dir.join("project.json")) {
        let pj: serde_json::Value = serde_json::from_str(&pj)?;
        if let Some(r) = pj["test_window"]["rect_pixels_top_left"].as_array() {
            let rect = [r[0].as_f64().unwrap(), r[1].as_f64().unwrap(), r[2].as_f64().unwrap(), r[3].as_f64().unwrap()];
            let m = region_mean(&rgba, w, rect);
            let is_magenta = m[0] > 200.0 && m[1] < 60.0 && m[2] > 200.0;
            report["test_window_region"] = json!({"rect_px": rect, "mean_rgb": m, "looks_magenta": is_magenta, "filter": pj["filter"]});
        }
    }

    let dur = pts.last().copied().unwrap_or(0.0) - base;
    let mut lat = vec![];
    let mut seed = 0x1234_5678u64;
    for _ in 0..seeks {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let s = (seed >> 11) as f64 / (1u64 << 53) as f64 * (dur - 0.1).max(0.0);
        let t = Instant::now();
        let r = open_reader(&file, Some((s + base, 0.5)), PIX_420V)?;
        let mut got = false;
        while let Some(sb) = r.next() {
            if unsafe { sb.image_buffer() }.is_some() {
                let imp = gpu.import_nv12(&sb)?;
                gpu.render(&imp, &preview);
                gpu.wait();
                got = true;
                break;
            }
        }
        if got {
            lat.push(t.elapsed().as_secs_f64() * 1e3);
        }
        unsafe { r.reader.cancelReading() };
    }
    report["random_seek_to_rendered_frame_ms"] = summarize(&lat);
    println!("{}", serde_json::to_string_pretty(&report)?);
    std::fs::write(dir.join(format!("{}.decode.json", file.file_stem().unwrap().to_string_lossy())), serde_json::to_vec_pretty(&report)?)?;
    Ok(())
}
