//! wgpu compositor. Every shader stage mirrors the integer maths in `maths.rs`.

use std::sync::Arc;
use std::sync::mpsc;

use sc_geometry::SizePx;
use sc_platform::{Error, Nv12View, Result};

use crate::cpu::{check_job, check_view};
use crate::maths::{self, CameraPlan, ScreenPlan};
use crate::surface::NativeSurface;
use crate::{AdapterSummary, FrameJob, OutputFormat, RenderedFrame};

const VS: &str = r#"
@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
  let x = f32((i << 1u) & 2u) * 2.0 - 1.0;
  let y = f32(i & 2u) * 2.0 - 1.0;
  return vec4<f32>(x, y, 0.0, 1.0);
}
"#;

const PREFILTER: &str = r#"
struct Pre { src: vec4<i32>, k: vec4<i32>, c0: vec4<i32>, c1: vec4<i32> };
@group(0) @binding(0) var<uniform> u: Pre;
@group(0) @binding(1) var ty: texture_2d<u32>;
@group(0) @binding(2) var tuv: texture_2d<u32>;

fn rgb(x: i32, y: i32) -> vec3<i32> {
  let yy = i32(textureLoad(ty, vec2<i32>(x, y), 0).r);
  let c = textureLoad(tuv, vec2<i32>(x >> 1u, y >> 1u), 0);
  let du = i32(c.r) - 128;
  let dv = i32(c.g) - 128;
  let l = u.c0.x * (yy - u.c0.y) + 32768;
  let r = (l + u.c0.z * dv) >> 16u;
  let g = (l - u.c0.w * du - u.c1.x * dv) >> 16u;
  let b = (l + u.c1.y * du) >> 16u;
  return clamp(vec3<i32>(r, g, b), vec3<i32>(0), vec3<i32>(255));
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
  let x0 = i32(pos.x) * u.k.x;
  let x1 = min(x0 + u.k.x, u.src.z);
  let y0 = i32(pos.y) * u.k.y;
  let y1 = min(y0 + u.k.y, u.src.w);
  var s = vec3<i32>(0);
  for (var y = y0; y < y1; y = y + 1) {
    for (var x = x0; x < x1; x = x + 1) {
      s = s + rgb(u.src.x + x, u.src.y + y);
    }
  }
  let n = (x1 - x0) * (y1 - y0);
  let o = (s + vec3<i32>(n / 2)) / vec3<i32>(n);
  return vec4<u32>(vec3<u32>(o), 255u);
}
"#;

const COMPOSITE: &str = r#"
struct Comp { dst: vec4<i32>, sk: vec4<i32>, p: vec4<i32> };
@group(0) @binding(0) var<uniform> u: Comp;
@group(0) @binding(1) var tp: texture_2d<u32>;

fn axis(rel: i32, s: i32, d: i32, k: i32, p: i32) -> vec3<i32> {
  let den = 2 * d * k;
  let n = (2 * rel + 1) * s - d * k;
  var i0 = n / den;
  if (i0 * den > n) {
    i0 = i0 - 1;
  }
  let rem = n - i0 * den;
  let w = (rem * 256 + d * k) / den;
  return vec3<i32>(clamp(i0, 0, p - 1), clamp(i0 + 1, 0, p - 1), w);
}

fn texel(x: i32, y: i32) -> vec3<i32> {
  return vec3<i32>(textureLoad(tp, vec2<i32>(x, y), 0).rgb);
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
  let ax = axis(i32(pos.x) - u.dst.x, u.sk.x, u.dst.z, u.sk.z, u.p.x);
  let ay = axis(i32(pos.y) - u.dst.y, u.sk.y, u.dst.w, u.sk.w, u.p.y);
  let wx = ax.z;
  let wy = ay.z;
  let top = texel(ax.x, ay.x) * (256 - wx) + texel(ax.y, ay.x) * wx;
  let bot = texel(ax.x, ay.y) * (256 - wx) + texel(ax.y, ay.y) * wx;
  let v = (top * (256 - wy) + bot * wy + vec3<i32>(32768)) >> vec3<u32>(16u);
  return vec4<f32>(vec3<f32>(v) / 255.0, 1.0);
}
"#;

const TO_I420: &str = r#"
@group(0) @binding(0) var tt: texture_2d<f32>;

fn px(x: i32, y: i32) -> vec3<i32> {
  return vec3<i32>(round(textureLoad(tt, vec2<i32>(x, y), 0).rgb * 255.0));
}

@fragment
fn fs_y(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
  let c = px(i32(pos.x), i32(pos.y));
  let y = clamp((11966 * c.r + 40254 * c.g + 4064 * c.b + 1081344) >> 16u, 0, 255);
  return vec4<u32>(u32(y), 0u, 0u, 1u);
}

struct Uv { @location(0) u: vec4<u32>, @location(1) v: vec4<u32> };

@fragment
fn fs_uv(@builtin(position) pos: vec4<f32>) -> Uv {
  let x = 2 * i32(pos.x);
  let y = 2 * i32(pos.y);
  let s = px(x, y) + px(x + 1, y) + px(x, y + 1) + px(x + 1, y + 1);
  let cb = clamp((-6596 * s.r - 22189 * s.g + 28785 * s.b + 33685504) >> 18u, 0, 255);
  let cr = clamp((28785 * s.r - 26145 * s.g - 2640 * s.b + 33685504) >> 18u, 0, 255);
  var o: Uv;
  o.u = vec4<u32>(u32(cb), 0u, 0u, 1u);
  o.v = vec4<u32>(u32(cr), 0u, 0u, 1u);
  return o;
}
"#;

const CAMERA: &str = r#"
struct Cam { dst: vec4<i32>, sk: vec4<i32>, p: vec4<i32>, misc: vec4<i32>, border: vec4<i32>, f: vec4<f32> };
@group(0) @binding(0) var<uniform> u: Cam;
@group(0) @binding(1) var tp: texture_2d<u32>;
@group(0) @binding(2) var tu: texture_2d<f32>;

fn axis(rel: i32, s: i32, d: i32, k: i32, p: i32) -> vec3<i32> {
  let den = 2 * d * k;
  let n = (2 * rel + 1) * s - d * k;
  var i0 = n / den;
  if (i0 * den > n) {
    i0 = i0 - 1;
  }
  let rem = n - i0 * den;
  let w = (rem * 256 + d * k) / den;
  return vec3<i32>(clamp(i0, 0, p - 1), clamp(i0 + 1, 0, p - 1), w);
}

fn texel(x: i32, y: i32) -> vec3<i32> {
  return vec3<i32>(textureLoad(tp, vec2<i32>(x, y), 0).rgb);
}

fn sdf(px: f32, py: f32, hw: f32, hh: f32, r: f32) -> f32 {
  let qx = abs(px) - hw + r;
  let qy = abs(py) - hh + r;
  let mx = max(qx, 0.0);
  let my = max(qy, 0.0);
  return sqrt(mx * mx + my * my) + min(max(qx, qy), 0.0) - r;
}

fn cov(d: f32) -> i32 {
  return i32(floor(clamp(0.5 - d, 0.0, 1.0) * 256.0 + 0.5));
}

fn shadow_a(ds: f32, blur: f32, strength: i32) -> i32 {
  let t = clamp((blur - ds) / (2.0 * blur), 0.0, 1.0);
  let s = t * t * (3.0 - 2.0 * t);
  return i32(floor(s * f32(strength) * 0.45 / 255.0 * 256.0 + 0.5));
}

fn mix256(a: i32, b: i32, w: i32) -> i32 {
  return (a * w + b * (256 - w) + 128) >> 8u;
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
  let x = i32(pos.x);
  let y = i32(pos.y);
  var c = vec4<i32>(round(textureLoad(tu, vec2<i32>(x, y), 0) * 255.0));
  let hw = f32(u.dst.z) * 0.5;
  let hh = f32(u.dst.w) * 0.5;
  let px = f32(x) + 0.5 - (f32(u.dst.x) + hw);
  let py = f32(y) + 0.5 - (f32(u.dst.y) + hh);
  let r = u.f.x;
  if (u.misc.y > 0) {
    let sa = shadow_a(sdf(px, py - f32(u.misc.z), hw, hh, r), u.f.z, u.misc.y);
    c = vec4<i32>(mix256(0, c.r, sa), mix256(0, c.g, sa), mix256(0, c.b, sa), mix256(255, c.a, sa));
  }
  let d = sdf(px, py, hw, hh, r);
  let k = cov(d);
  if (k > 0) {
    var rx = x - u.dst.x;
    if (u.misc.x != 0) {
      rx = u.dst.z - 1 - rx;
    }
    let ax = axis(rx, u.sk.x, u.dst.z, u.sk.z, u.p.x);
    let ay = axis(y - u.dst.y, u.sk.y, u.dst.w, u.sk.w, u.p.y);
    let top = texel(ax.x, ay.x) * (256 - ax.z) + texel(ax.y, ay.x) * ax.z;
    let bot = texel(ax.x, ay.y) * (256 - ax.z) + texel(ax.y, ay.y) * ax.z;
    var v = (top * (256 - ay.z) + bot * ay.z + vec3<i32>(32768)) >> vec3<u32>(16u);
    if (u.misc.w != 0) {
      let ci = cov(d + u.f.y);
      v = vec3<i32>(mix256(v.r, u.border.r, ci), mix256(v.g, u.border.g, ci), mix256(v.b, u.border.b, ci));
    }
    c = vec4<i32>(mix256(v.r, c.r, k), mix256(v.g, c.g, k), mix256(v.b, c.b, k), mix256(255, c.a, k));
  }
  return vec4<f32>(c) / 255.0;
}
"#;

const TEXT: &str = r#"
struct Txt { inv0: vec4<f32>, inv1: vec4<f32>, size: vec4<i32> };
@group(0) @binding(0) var<uniform> u: Txt;
@group(0) @binding(1) var tt: texture_2d<u32>;
@group(0) @binding(2) var under: texture_2d<f32>;

fn texel(x: i32, y: i32) -> vec4<i32> {
  if (x < 0 || y < 0 || x >= u.size.x || y >= u.size.y) {
    return vec4<i32>(0);
  }
  return vec4<i32>(textureLoad(tt, vec2<i32>(x, y), 0));
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
  let x = i32(pos.x);
  let y = i32(pos.y);
  let px = f32(x) + 0.5;
  let py = f32(y) + 0.5;
  let uu = u.inv0.x * px + u.inv0.z * py + u.inv1.x;
  let vv = u.inv0.y * px + u.inv0.w * py + u.inv1.y;
  let fx = i32(floor(uu * 256.0 + 0.5)) - 128;
  let fy = i32(floor(vv * 256.0 + 0.5)) - 128;
  let tx = fx >> 8u;
  let ty = fy >> 8u;
  let wx = fx & 255;
  let wy = fy & 255;
  let top = texel(tx, ty) * (256 - wx) + texel(tx + 1, ty) * wx;
  let bot = texel(tx, ty + 1) * (256 - wx) + texel(tx + 1, ty + 1) * wx;
  let v = (top * (256 - wy) + bot * wy + vec4<i32>(32768)) >> vec4<u32>(16u);
  let s = (v * u.size.z + vec4<i32>(128)) >> vec4<u32>(8u);
  let d = vec4<i32>(round(textureLoad(under, vec2<i32>(x, y), 0) * 255.0));
  let o = s + (d * (255 - s.a) + vec4<i32>(127)) / 255;
  return vec4<f32>(clamp(vec4<f32>(o), vec4<f32>(0.0), vec4<f32>(255.0)) / 255.0);
}
"#;

const BLIT: &str = r#"
@group(0) @binding(0) var<uniform> origin: vec4<i32>;
@group(0) @binding(1) var src: texture_2d<f32>;

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
  return textureLoad(src, vec2<i32>(pos.xy) - origin.xy, 0);
}
"#;

const TARGET: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const ROW_ALIGN: u32 = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;

struct Tex {
    tex: wgpu::Texture,
    view: wgpu::TextureView,
}

pub(crate) struct Gpu {
    #[cfg(target_os = "macos")]
    instance: wgpu::Instance,
    #[cfg(target_os = "macos")]
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    blit: Option<(wgpu::TextureFormat, wgpu::RenderPipeline)>,
    blit_ubuf: wgpu::Buffer,
    summary: AdapterSummary,
    max_dim: u32,
    prefilter: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    to_y: wgpu::RenderPipeline,
    to_uv: wgpu::RenderPipeline,
    camera: wgpu::RenderPipeline,
    text: wgpu::RenderPipeline,
    text_tex: std::collections::HashMap<u64, Tex>,
    pre_ubuf: wgpu::Buffer,
    comp_ubuf: wgpu::Buffer,
    cam_pre_ubuf: wgpu::Buffer,
    cam_ubuf: wgpu::Buffer,
    frame_y: Option<Tex>,
    frame_uv: Option<Tex>,
    pre: Option<Tex>,
    cam_y: Option<Tex>,
    cam_uv: Option<Tex>,
    cam_pre: Option<Tex>,
    under: Option<Tex>,
    target: Option<Tex>,
    bg: Option<Tex>,
    bg_key: Option<u64>,
    last_screen: Option<(sc_platform::FrameBuffer, bool)>,
    cam_imported: Option<(Tex, Tex)>,
    pub(crate) camera_in_place: u64,
    #[cfg(target_os = "macos")]
    cv_cache: Option<crate::iosurface::TextureCache>,
    out_y: Option<Tex>,
    out_u: Option<Tex>,
    out_v: Option<Tex>,
    readback: Option<wgpu::Buffer>,
}

/// Render threads are plain threads with no autorelease pool, so the Metal and CoreVideo objects
/// a frame autoreleases would otherwise live as long as the thread.
#[cfg(target_os = "macos")]
fn drain_autoreleased<T>(f: impl FnOnce() -> T) -> T {
    objc2::rc::autoreleasepool(|_| f())
}

#[cfg(not(target_os = "macos"))]
fn drain_autoreleased<T>(f: impl FnOnce() -> T) -> T {
    f()
}

fn other(what: &str, e: impl std::fmt::Display) -> Error {
    Error::Other(format!("sc_render: {what}: {e}"))
}

pub(crate) fn summarize(info: &wgpu::AdapterInfo) -> AdapterSummary {
    let lname = info.name.to_ascii_lowercase();
    let software = info.device_type == wgpu::DeviceType::Cpu
        || [
            "llvmpipe",
            "lavapipe",
            "swiftshader",
            "warp",
            "basic render",
        ]
        .iter()
        .any(|n| lname.contains(n));
    AdapterSummary {
        name: info.name.clone(),
        backend: format!("{:?}", info.backend),
        device_type: format!("{:?}", info.device_type),
        software,
    }
}

/// `Ok(None)` only for `Auto` with no usable adapter.
pub(crate) fn open(backends: wgpu::Backends, auto: bool) -> Result<Option<Gpu>> {
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = backends;
    let instance = wgpu::Instance::new(desc);
    let hardware = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    }));
    let mut tried = Vec::new();
    let mut candidates = Vec::new();
    match hardware {
        Ok(a) if !summarize(&a.get_info()).software => candidates.push(a),
        Ok(a) => {
            let fallback =
                pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                    force_fallback_adapter: true,
                    ..Default::default()
                }));
            candidates.extend(fallback.ok());
            candidates.push(a);
        }
        Err(e) => {
            tried.push(format!("hardware: {e}"));
            match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                force_fallback_adapter: true,
                ..Default::default()
            })) {
                Ok(a) => candidates.push(a),
                Err(e) => tried.push(format!("software: {e}")),
            }
        }
    }
    for adapter in candidates {
        let info = adapter.get_info();
        match Gpu::new(&instance, &adapter) {
            Ok(gpu) => {
                tracing::info!(adapter = %info.name, backend = ?info.backend, "sc_render: wgpu adapter");
                return Ok(Some(gpu));
            }
            Err(e) => tried.push(format!("{} ({:?}): {e}", info.name, info.backend)),
        }
    }
    if auto {
        tracing::warn!(tried = ?tried, "sc_render: no wgpu adapter, using the CPU renderer");
        return Ok(None);
    }
    Err(Error::Other(format!(
        "sc_render: no wgpu adapter for {backends:?}: {}",
        tried.join("; ")
    )))
}

fn pipeline(
    device: &wgpu::Device,
    label: &str,
    fs_src: &str,
    entry: &str,
    targets: &[Option<wgpu::ColorTargetState>],
) -> wgpu::RenderPipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(format!("{VS}{fs_src}").into()),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: None,
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            targets,
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

fn uint_target(format: wgpu::TextureFormat) -> Option<wgpu::ColorTargetState> {
    Some(wgpu::ColorTargetState {
        format,
        blend: None,
        write_mask: wgpu::ColorWrites::ALL,
    })
}

fn ensure_tex(
    device: &wgpu::Device,
    slot: &mut Option<Tex>,
    label: &str,
    size: (u32, u32),
    format: wgpu::TextureFormat,
    usage: wgpu::TextureUsages,
) {
    if let Some(t) = slot
        && t.tex.width() == size.0
        && t.tex.height() == size.1
    {
        return;
    }
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    });
    let view = tex.create_view(&Default::default());
    *slot = Some(Tex { tex, view });
}

fn tex_of(slot: &Option<Tex>) -> Result<&Tex> {
    slot.as_ref()
        .ok_or_else(|| Error::Other("sc_render: internal texture missing".into()))
}

fn upload_nv12(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    (y_slot, uv_slot): (&mut Option<Tex>, &mut Option<Tex>),
    label: &str,
    view: &Nv12View<'_>,
) -> Result<()> {
    let (w, h) = (view.size.w, view.size.h);
    let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
    let usage = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST;
    ensure_tex(
        device,
        y_slot,
        &format!("{label} y"),
        (w, h),
        wgpu::TextureFormat::R8Uint,
        usage,
    );
    ensure_tex(
        device,
        uv_slot,
        &format!("{label} uv"),
        (cw, ch),
        wgpu::TextureFormat::Rg8Uint,
        usage,
    );
    queue.write_texture(
        tex_of(y_slot)?.tex.as_image_copy(),
        &view.y[..view.y_stride * (h as usize - 1) + w as usize],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(view.y_stride as u32),
            rows_per_image: Some(h),
        },
        extent(w, h),
    );
    queue.write_texture(
        tex_of(uv_slot)?.tex.as_image_copy(),
        &view.uv[..view.uv_stride * (ch as usize - 1) + 2 * cw as usize],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(view.uv_stride as u32),
            rows_per_image: Some(ch),
        },
        extent(cw, ch),
    );
    Ok(())
}

/// The part of `view` that `layer.src` reads, on even pixels so chroma stays aligned, and the
/// layer with `src` relative to it: only that part is uploaded.
fn crop_to_source<'v, 'l>(
    view: &Nv12View<'v>,
    layer: &crate::CameraLayer<'l>,
) -> (Nv12View<'v>, crate::CameraLayer<'l>) {
    let (fw, fh) = (view.size.w as i64, view.size.h as i64);
    let s = layer.src;
    let x0 = (i64::from(s.x).clamp(0, fw) & !1) as usize;
    let y0 = (i64::from(s.y).clamp(0, fh) & !1) as usize;
    let x1 = ((i64::from(s.x) + i64::from(s.w) + 1) & !1).clamp(x0 as i64, fw) as usize;
    let y1 = ((i64::from(s.y) + i64::from(s.h) + 1) & !1).clamp(y0 as i64, fh) as usize;
    if x1 <= x0 || y1 <= y0 {
        return (*view, layer.clone());
    }
    let sub = Nv12View {
        size: SizePx {
            w: (x1 - x0) as u32,
            h: (y1 - y0) as u32,
        },
        y: &view.y[y0 * view.y_stride + x0..],
        y_stride: view.y_stride,
        uv: &view.uv[(y0 / 2) * view.uv_stride + x0..],
        uv_stride: view.uv_stride,
        full_range: view.full_range,
    };
    let mut local = layer.clone();
    local.src.x -= x0 as i32;
    local.src.y -= y0 as i32;
    (sub, local)
}

fn i32s(v: &[i32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_ne_bytes()).collect()
}

fn pass<'e>(
    enc: &'e mut wgpu::CommandEncoder,
    views: &[&wgpu::TextureView],
    load: bool,
) -> wgpu::RenderPass<'e> {
    let attachments: Vec<Option<wgpu::RenderPassColorAttachment>> = views
        .iter()
        .map(|v| {
            Some(wgpu::RenderPassColorAttachment {
                view: v,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: if load {
                        wgpu::LoadOp::Load
                    } else {
                        wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                    },
                    store: wgpu::StoreOp::Store,
                },
            })
        })
        .collect();
    enc.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: None,
        color_attachments: &attachments,
        ..Default::default()
    })
}

impl Gpu {
    fn new(instance: &wgpu::Instance, adapter: &wgpu::Adapter) -> Result<Gpu> {
        #[cfg(not(target_os = "macos"))]
        let _ = instance;
        let limits = adapter.limits();
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("sc_render"),
            required_limits: limits.clone(),
            ..Default::default()
        }))
        .map_err(|e| other("request_device", e))?;
        device.on_uncaptured_error(Arc::new(|e| {
            tracing::error!(error = %e, "sc_render: uncaptured wgpu error");
        }));
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let blend = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        };
        let prefilter = pipeline(
            &device,
            "sc_render prefilter",
            PREFILTER,
            "fs",
            &[uint_target(wgpu::TextureFormat::Rgba8Uint)],
        );
        let composite = pipeline(
            &device,
            "sc_render composite",
            COMPOSITE,
            "fs",
            &[Some(wgpu::ColorTargetState {
                format: TARGET,
                blend: Some(wgpu::BlendState {
                    color: blend,
                    alpha: blend,
                }),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        );
        let to_y = pipeline(
            &device,
            "sc_render to_y",
            TO_I420,
            "fs_y",
            &[uint_target(wgpu::TextureFormat::R8Uint)],
        );
        let to_uv = pipeline(
            &device,
            "sc_render to_uv",
            TO_I420,
            "fs_uv",
            &[
                uint_target(wgpu::TextureFormat::R8Uint),
                uint_target(wgpu::TextureFormat::R8Uint),
            ],
        );
        let camera = pipeline(
            &device,
            "sc_render camera",
            CAMERA,
            "fs",
            &[Some(wgpu::ColorTargetState {
                format: TARGET,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        );
        let text = pipeline(
            &device,
            "sc_render text",
            TEXT,
            "fs",
            &[Some(wgpu::ColorTargetState {
                format: TARGET,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        );
        let ubuf = |label| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: 96,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let pre_ubuf = ubuf("sc_render pre uniforms");
        let comp_ubuf = ubuf("sc_render comp uniforms");
        let cam_pre_ubuf = ubuf("sc_render camera pre uniforms");
        let cam_ubuf = ubuf("sc_render camera uniforms");
        let blit_ubuf = ubuf("sc_render blit uniforms");
        if let Some(e) = pollster::block_on(scope.pop()) {
            return Err(other("pipeline setup", e));
        }
        #[cfg(target_os = "macos")]
        let cv_cache = crate::iosurface::TextureCache::new(&device);
        Ok(Gpu {
            summary: summarize(&adapter.get_info()),
            max_dim: limits.max_texture_dimension_2d,
            #[cfg(target_os = "macos")]
            instance: instance.clone(),
            #[cfg(target_os = "macos")]
            adapter: adapter.clone(),
            device,
            queue,
            blit: None,
            blit_ubuf,
            prefilter,
            composite,
            to_y,
            to_uv,
            camera,
            text,
            text_tex: std::collections::HashMap::new(),
            pre_ubuf,
            comp_ubuf,
            cam_pre_ubuf,
            cam_ubuf,
            frame_y: None,
            frame_uv: None,
            pre: None,
            cam_y: None,
            cam_uv: None,
            cam_pre: None,
            under: None,
            target: None,
            bg: None,
            bg_key: None,
            last_screen: None,
            cam_imported: None,
            camera_in_place: 0,
            #[cfg(target_os = "macos")]
            cv_cache,
            out_y: None,
            out_u: None,
            out_v: None,
            readback: None,
        })
    }

    pub(crate) fn summary(&self) -> &AdapterSummary {
        &self.summary
    }

    pub(crate) fn render(&mut self, job: &FrameJob, out: OutputFormat) -> Result<RenderedFrame> {
        check_job(job, out)?;
        self.fits(job.canvas)?;
        self.scoped(|gpu| gpu.render_inner(job, out))
    }

    pub(crate) fn present(
        &mut self,
        job: &FrameJob,
        surface: &mut NativeSurface,
        drawable: SizePx,
        at: (u32, u32),
        clear: [u8; 3],
    ) -> Result<bool> {
        check_job(job, OutputFormat::Rgba8)?;
        self.fits(job.canvas)?;
        maths::check_size("drawable", drawable)?;
        let canvas = job.canvas;
        if u64::from(at.0) + u64::from(canvas.w) > u64::from(drawable.w)
            || u64::from(at.1) + u64::from(canvas.h) > u64::from(drawable.h)
        {
            return Err(Error::Other(format!(
                "sc_render: canvas {}x{} at {at:?} does not fit the {}x{} drawable",
                canvas.w, canvas.h, drawable.w, drawable.h
            )));
        }
        self.fits(drawable)?;
        self.scoped(|gpu| gpu.present_inner(job, surface, drawable, at, clear))
    }

    fn fits(&self, size: SizePx) -> Result<()> {
        if size.w > self.max_dim || size.h > self.max_dim {
            return Err(Error::Other(format!(
                "sc_render: {}x{} exceeds the adapter's {} px texture limit",
                size.w, size.h, self.max_dim
            )));
        }
        Ok(())
    }

    fn scoped<T>(&mut self, f: impl FnOnce(&mut Gpu) -> Result<T>) -> Result<T> {
        drain_autoreleased(|| {
            let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
            let oom = self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
            let result = f(self);
            let oom = pollster::block_on(oom.pop());
            let validation = pollster::block_on(scope.pop());
            if let Some(e) = validation.or(oom) {
                return Err(other("wgpu", e));
            }
            result
        })
    }

    fn render_inner(&mut self, job: &FrameJob, out: OutputFormat) -> Result<RenderedFrame> {
        let mut enc = self.compose(job)?;
        let (copies, total) = self.encode_output(&mut enc, job.canvas, out)?;
        self.queue.submit([enc.finish()]);
        let data = self.read(&copies, total);
        self.release_camera();
        Ok(RenderedFrame {
            size: job.canvas,
            format: out,
            data: data?,
        })
    }

    /// # Safety
    /// `layer` points to a live `CAMetalLayer`.
    #[cfg(target_os = "macos")]
    pub(crate) unsafe fn metal_layer_surface(
        &self,
        layer: std::ptr::NonNull<std::ffi::c_void>,
    ) -> Result<NativeSurface> {
        // SAFETY: a live CAMetalLayer, which wgpu retains for the surface's lifetime.
        let surface = unsafe {
            self.instance
                .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(
                    layer.as_ptr(),
                ))
        }
        .map_err(|e| other("create_surface", e))?;
        NativeSurface::new(surface, &self.adapter)
    }

    fn present_inner(
        &mut self,
        job: &FrameJob,
        surface: &mut NativeSurface,
        drawable: SizePx,
        at: (u32, u32),
        clear: [u8; 3],
    ) -> Result<bool> {
        let Some(frame) = surface.acquire(&self.device, drawable)? else {
            return Ok(false);
        };
        let mut enc = self.compose(job)?;
        let format = surface.format;
        if self.blit.as_ref().is_none_or(|(f, _)| *f != format) {
            let target = wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            };
            let p = pipeline(&self.device, "sc_render blit", BLIT, "fs", &[Some(target)]);
            self.blit = Some((format, p));
        }
        let Some((_, blit)) = &self.blit else {
            return Err(Error::Other("sc_render: blit pipeline missing".into()));
        };
        let origin = [at.0 as i32, at.1 as i32, 0, 0];
        self.queue.write_buffer(&self.blit_ubuf, 0, &i32s(&origin));
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &blit.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.blit_ubuf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&tex_of(&self.target)?.view),
                },
            ],
        });
        let view = frame.texture.create_view(&Default::default());
        let [r, g, b] = clear.map(|c| f64::from(c) / 255.0);
        {
            let mut p = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("sc_render present"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r, g, b, a: 1.0 }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            let canvas = job.canvas;
            p.set_pipeline(blit);
            p.set_bind_group(0, &bind, &[]);
            p.set_viewport(
                at.0 as f32,
                at.1 as f32,
                canvas.w as f32,
                canvas.h as f32,
                0.0,
                1.0,
            );
            p.draw(0..3, 0..1);
        }
        self.queue.submit([enc.finish()]);
        self.queue.present(frame);
        self.release_camera();
        Ok(true)
    }

    fn release_camera(&mut self) {
        if self.cam_imported.take().is_some() {
            #[cfg(target_os = "macos")]
            if let Some(cache) = &self.cv_cache {
                cache.flush();
            }
        }
    }

    /// Background, screen and camera into `self.target`, recorded but not submitted.
    fn compose(&mut self, job: &FrameJob) -> Result<wgpu::CommandEncoder> {
        let canvas = job.canvas;
        let rw = wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST;
        ensure_tex(
            &self.device,
            &mut self.target,
            "sc_render target",
            (canvas.w, canvas.h),
            TARGET,
            rw,
        );
        let bg = job.background;
        let layout = wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(bg.stride as u32),
            rows_per_image: Some(canvas.h),
        };
        let mut bg_copy = false;
        match job.background_key {
            Some(key) => {
                let same = self.bg_key == Some(key)
                    && self
                        .bg
                        .as_ref()
                        .is_some_and(|t| (t.tex.width(), t.tex.height()) == (canvas.w, canvas.h));
                if !same {
                    ensure_tex(
                        &self.device,
                        &mut self.bg,
                        "sc_render background",
                        (canvas.w, canvas.h),
                        TARGET,
                        wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
                    );
                    self.queue.write_texture(
                        tex_of(&self.bg)?.tex.as_image_copy(),
                        &bg.data,
                        layout,
                        extent(canvas.w, canvas.h),
                    );
                    self.bg_key = Some(key);
                }
                bg_copy = true;
            }
            None => {
                self.bg_key = None;
                self.queue.write_texture(
                    tex_of(&self.target)?.tex.as_image_copy(),
                    &bg.data,
                    layout,
                    extent(canvas.w, canvas.h),
                );
            }
        }

        let mut plan = None;
        if let Some(layer) = &job.screen {
            let reused = self
                .last_screen
                .as_ref()
                .filter(|(f, _)| Arc::ptr_eq(&f.0, &layer.frame.0) && self.frame_y.is_some())
                .map(|(_, full)| *full);
            if let Some(full_range) = reused {
                plan = maths::plan_screen(
                    canvas,
                    layer.frame.0.size(),
                    full_range,
                    layer.src,
                    layer.dst,
                )?;
            } else {
                let mut result = Ok(None);
                let mut full_range = false;
                self.last_screen = None;
                layer.frame.0.with_nv12(&mut |view| {
                    full_range = view.full_range;
                    result = self.upload_frame(canvas, layer.src, layer.dst, &view);
                })?;
                plan = result?;
                if plan.is_some() {
                    self.last_screen = Some((layer.frame.clone(), full_range));
                }
            }
        }

        self.cam_imported = None;
        let cam_plan = match &job.camera {
            Some(layer) => self.camera_plan(canvas, layer)?,
            None => None,
        };

        let mut enc = self.device.create_command_encoder(&Default::default());
        if bg_copy {
            enc.copy_texture_to_texture(
                tex_of(&self.bg)?.tex.as_image_copy(),
                tex_of(&self.target)?.tex.as_image_copy(),
                extent(canvas.w, canvas.h),
            );
        }
        if let Some(plan) = &plan {
            self.encode_screen(&mut enc, plan)?;
        }
        if let Some(plan) = &cam_plan {
            self.encode_camera(&mut enc, canvas, plan)?;
        }
        self.encode_text(&mut enc, canvas, job.text)?;
        Ok(enc)
    }

    fn encode_text(
        &mut self,
        enc: &mut wgpu::CommandEncoder,
        canvas: SizePx,
        layers: &[crate::TextLayer<'_>],
    ) -> Result<()> {
        let keep: std::collections::HashSet<u64> = layers.iter().map(|l| l.key).collect();
        self.text_tex.retain(|k, _| keep.contains(k));
        if layers.is_empty() {
            return Ok(());
        }
        ensure_tex(
            &self.device,
            &mut self.under,
            "sc_render under camera",
            (canvas.w, canvas.h),
            TARGET,
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        );
        for layer in layers {
            let Some(plan) = maths::plan_text(canvas, layer) else {
                continue;
            };
            let img = layer.image;
            if !self.text_tex.contains_key(&layer.key) {
                let tex = self.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("sc_render text layer"),
                    size: extent(img.size.w, img.size.h),
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Uint,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                self.queue.write_texture(
                    tex.as_image_copy(),
                    &img.data,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(img.stride as u32),
                        rows_per_image: Some(img.size.h),
                    },
                    extent(img.size.w, img.size.h),
                );
                let view = tex.create_view(&Default::default());
                self.text_tex.insert(layer.key, Tex { tex, view });
            }
            let mut uniforms = Vec::with_capacity(48);
            for f in [
                plan.inv[0],
                plan.inv[1],
                plan.inv[2],
                plan.inv[3],
                plan.inv[4],
                plan.inv[5],
                0.0,
                0.0,
            ] {
                uniforms.extend_from_slice(&f.to_ne_bytes());
            }
            uniforms.extend_from_slice(&i32s(&[plan.size[0], plan.size[1], plan.op256, 0]));
            let ubuf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("sc_render text uniforms"),
                size: uniforms.len() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.queue.write_buffer(&ubuf, 0, &uniforms);
            let tex = self
                .text_tex
                .get(&layer.key)
                .ok_or_else(|| Error::Other("sc_render: text texture missing".into()))?;
            let under = tex_of(&self.under)?;
            let target = tex_of(&self.target)?;
            let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.text.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: ubuf.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&tex.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&under.view),
                    },
                ],
            });
            enc.copy_texture_to_texture(
                target.tex.as_image_copy(),
                under.tex.as_image_copy(),
                extent(canvas.w, canvas.h),
            );
            let [x0, y0, x1, y1] = plan.region;
            let mut p = pass(enc, &[&target.view], true);
            p.set_pipeline(&self.text);
            p.set_bind_group(0, &bind, &[]);
            p.set_scissor_rect(x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32);
            p.draw(0..3, 0..1);
        }
        Ok(())
    }

    fn upload_frame(
        &mut self,
        canvas: SizePx,
        src: sc_geometry::RectPx,
        dst: sc_geometry::RectPx,
        view: &Nv12View<'_>,
    ) -> Result<Option<ScreenPlan>> {
        maths::check_size("frame", view.size)?;
        check_view(view)?;
        if view.size.w > self.max_dim || view.size.h > self.max_dim {
            return Err(Error::Other(format!(
                "sc_render: frame {}x{} exceeds the adapter's {} px texture limit",
                view.size.w, view.size.h, self.max_dim
            )));
        }
        let Some(plan) = maths::plan_screen(canvas, view.size, view.full_range, src, dst)? else {
            return Ok(None);
        };
        upload_nv12(
            &self.device,
            &self.queue,
            (&mut self.frame_y, &mut self.frame_uv),
            "sc_render frame",
            view,
        )?;
        Ok(Some(plan))
    }

    fn encode_screen(&mut self, enc: &mut wgpu::CommandEncoder, plan: &ScreenPlan) -> Result<()> {
        let c = maths::coeffs(plan.full_range);
        let [sx, sy, sw, sh] = plan.src;
        let [kx, ky] = plan.k;
        let [pw, ph] = plan.p;
        let [dx, dy, dw, dh] = plan.dst;
        self.queue.write_buffer(
            &self.pre_ubuf,
            0,
            &i32s(&[
                sx, sy, sw, sh, kx, ky, 0, 0, c.y_mul, c.y_off, c.rv, c.gu, c.gv, c.bu, 0, 0,
            ]),
        );
        self.queue.write_buffer(
            &self.comp_ubuf,
            0,
            &i32s(&[dx, dy, dw, dh, sw, sh, kx, ky, pw, ph, 0, 0, 0, 0, 0, 0]),
        );
        ensure_tex(
            &self.device,
            &mut self.pre,
            "sc_render prefiltered",
            (pw as u32, ph as u32),
            wgpu::TextureFormat::Rgba8Uint,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        );
        let pre = tex_of(&self.pre)?;
        let pre_bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.prefilter.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.pre_ubuf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&tex_of(&self.frame_y)?.view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&tex_of(&self.frame_uv)?.view),
                },
            ],
        });
        let comp_bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.composite.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.comp_ubuf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&pre.view),
                },
            ],
        });
        {
            let mut p = pass(enc, &[&pre.view], false);
            p.set_pipeline(&self.prefilter);
            p.set_bind_group(0, &pre_bind, &[]);
            p.draw(0..3, 0..1);
        }
        {
            let [x0, y0, x1, y1] = plan.clip;
            let mut p = pass(enc, &[&tex_of(&self.target)?.view], true);
            p.set_pipeline(&self.composite);
            p.set_bind_group(0, &comp_bind, &[]);
            p.set_scissor_rect(x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32);
            p.draw(0..3, 0..1);
        }
        Ok(())
    }

    fn camera_plan(
        &mut self,
        canvas: SizePx,
        layer: &crate::CameraLayer<'_>,
    ) -> Result<Option<CameraPlan>> {
        #[cfg(target_os = "macos")]
        if let Some(planned) = self.import_camera(canvas, layer)? {
            return Ok(planned);
        }
        let mut result = Ok(None);
        layer.frame.0.with_nv12(&mut |view| {
            result = self.upload_camera(canvas, layer, &view);
        })?;
        result
    }

    /// `None` when the frame has no IOSurface to read in place; it is uploaded instead.
    #[cfg(target_os = "macos")]
    fn import_camera(
        &mut self,
        canvas: SizePx,
        layer: &crate::CameraLayer<'_>,
    ) -> Result<Option<Option<CameraPlan>>> {
        let (Some(cache), Some(pb)) = (&self.cv_cache, layer.frame.0.cv_pixel_buffer()) else {
            return Ok(None);
        };
        let size = layer.frame.0.size();
        if size.w > self.max_dim || size.h > self.max_dim {
            return Ok(None);
        }
        maths::check_size("camera frame", size)?;
        // SAFETY: `layer.frame` owns the pixel buffer and outlives this render.
        let Some(planes) = (unsafe { cache.import(&self.device, pb, size) }) else {
            return Ok(None);
        };
        let plan = maths::plan_camera(canvas, size, planes.full_range, layer)?;
        let tex = |tex: wgpu::Texture| Tex {
            view: tex.create_view(&Default::default()),
            tex,
        };
        if plan.is_some() {
            self.cam_imported = Some((tex(planes.y), tex(planes.uv)));
            self.camera_in_place += 1;
        }
        Ok(Some(plan))
    }

    fn upload_camera(
        &mut self,
        canvas: SizePx,
        layer: &crate::CameraLayer<'_>,
        view: &Nv12View<'_>,
    ) -> Result<Option<CameraPlan>> {
        maths::check_size("camera frame", view.size)?;
        check_view(view)?;
        if view.size.w > self.max_dim || view.size.h > self.max_dim {
            return Err(Error::Other(format!(
                "sc_render: camera frame {}x{} exceeds the adapter's {} px texture limit",
                view.size.w, view.size.h, self.max_dim
            )));
        }
        let (sub, local) = crop_to_source(view, layer);
        let Some(plan) = maths::plan_camera(canvas, sub.size, sub.full_range, &local)? else {
            return Ok(None);
        };
        upload_nv12(
            &self.device,
            &self.queue,
            (&mut self.cam_y, &mut self.cam_uv),
            "sc_render camera",
            &sub,
        )?;
        Ok(Some(plan))
    }

    fn encode_camera(
        &mut self,
        enc: &mut wgpu::CommandEncoder,
        canvas: SizePx,
        plan: &CameraPlan,
    ) -> Result<()> {
        let s = &plan.sample;
        let c = maths::coeffs(s.full_range);
        let [sx, sy, sw, sh] = s.src;
        let [kx, ky] = s.k;
        let [pw, ph] = s.p;
        let [dx, dy, dw, dh] = s.dst;
        self.queue.write_buffer(
            &self.cam_pre_ubuf,
            0,
            &i32s(&[
                sx, sy, sw, sh, kx, ky, 0, 0, c.y_mul, c.y_off, c.rv, c.gu, c.gv, c.bu, 0, 0,
            ]),
        );
        let border = plan.border_rgb.unwrap_or([0; 3]);
        let mut uniforms = i32s(&[
            dx,
            dy,
            dw,
            dh,
            sw,
            sh,
            kx,
            ky,
            pw,
            ph,
            0,
            0,
            i32::from(plan.mirror),
            plan.shadow,
            plan.shadow_dy,
            i32::from(plan.border_rgb.is_some()),
            border[0],
            border[1],
            border[2],
            0,
        ]);
        for f in [plan.radius, plan.border_px, plan.shadow_blur, 0.0f32] {
            uniforms.extend_from_slice(&f.to_ne_bytes());
        }
        self.queue.write_buffer(&self.cam_ubuf, 0, &uniforms);
        ensure_tex(
            &self.device,
            &mut self.cam_pre,
            "sc_render camera prefiltered",
            (pw as u32, ph as u32),
            wgpu::TextureFormat::Rgba8Uint,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        );
        ensure_tex(
            &self.device,
            &mut self.under,
            "sc_render under camera",
            (canvas.w, canvas.h),
            TARGET,
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        );
        let cam_pre = tex_of(&self.cam_pre)?;
        let under = tex_of(&self.under)?;
        let target = tex_of(&self.target)?;
        let (cam_y, cam_uv) = match &self.cam_imported {
            Some((y, uv)) => (y, uv),
            None => (tex_of(&self.cam_y)?, tex_of(&self.cam_uv)?),
        };
        let pre_bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.prefilter.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.cam_pre_ubuf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&cam_y.view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&cam_uv.view),
                },
            ],
        });
        let cam_bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.camera.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.cam_ubuf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&cam_pre.view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&under.view),
                },
            ],
        });
        {
            let mut p = pass(enc, &[&cam_pre.view], false);
            p.set_pipeline(&self.prefilter);
            p.set_bind_group(0, &pre_bind, &[]);
            p.draw(0..3, 0..1);
        }
        enc.copy_texture_to_texture(
            target.tex.as_image_copy(),
            under.tex.as_image_copy(),
            extent(canvas.w, canvas.h),
        );
        {
            let [x0, y0, x1, y1] = plan.region;
            let mut p = pass(enc, &[&target.view], true);
            p.set_pipeline(&self.camera);
            p.set_bind_group(0, &cam_bind, &[]);
            p.set_scissor_rect(x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32);
            p.draw(0..3, 0..1);
        }
        Ok(())
    }

    fn encode_output(
        &mut self,
        enc: &mut wgpu::CommandEncoder,
        canvas: SizePx,
        out: OutputFormat,
    ) -> Result<(Vec<Plane>, u64)> {
        let (w, h) = (canvas.w, canvas.h);
        let mut copies = Vec::new();
        let mut offset = 0u64;
        let mut add = |copies: &mut Vec<Plane>, row: u32, rows: u32| {
            let bpr = row.div_ceil(ROW_ALIGN) * ROW_ALIGN;
            copies.push(Plane {
                offset,
                bpr,
                row,
                rows,
            });
            offset += u64::from(bpr) * u64::from(rows);
        };
        let target = tex_of(&self.target)?.tex.clone();
        match out {
            OutputFormat::Rgba8 => {
                add(&mut copies, w * 4, h);
                self.ensure_readback(offset);
                self.copy_out(enc, &target, &copies[0])?;
            }
            OutputFormat::I420Bt709Limited => {
                let usage = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC;
                let r8 = wgpu::TextureFormat::R8Uint;
                ensure_tex(
                    &self.device,
                    &mut self.out_y,
                    "sc_render y",
                    (w, h),
                    r8,
                    usage,
                );
                ensure_tex(
                    &self.device,
                    &mut self.out_u,
                    "sc_render u",
                    (w / 2, h / 2),
                    r8,
                    usage,
                );
                ensure_tex(
                    &self.device,
                    &mut self.out_v,
                    "sc_render v",
                    (w / 2, h / 2),
                    r8,
                    usage,
                );
                let target = tex_of(&self.target)?;
                let bind = |pipe: &wgpu::RenderPipeline| {
                    self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: None,
                        layout: &pipe.get_bind_group_layout(0),
                        entries: &[wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&target.view),
                        }],
                    })
                };
                let (y_bind, uv_bind) = (bind(&self.to_y), bind(&self.to_uv));
                let (ty, tu, tv) = (
                    tex_of(&self.out_y)?,
                    tex_of(&self.out_u)?,
                    tex_of(&self.out_v)?,
                );
                {
                    let mut p = pass(enc, &[&ty.view], false);
                    p.set_pipeline(&self.to_y);
                    p.set_bind_group(0, &y_bind, &[]);
                    p.draw(0..3, 0..1);
                }
                {
                    let mut p = pass(enc, &[&tu.view, &tv.view], false);
                    p.set_pipeline(&self.to_uv);
                    p.set_bind_group(0, &uv_bind, &[]);
                    p.draw(0..3, 0..1);
                }
                add(&mut copies, w, h);
                add(&mut copies, w / 2, h / 2);
                add(&mut copies, w / 2, h / 2);
                self.ensure_readback(offset);
                let (ty, tu, tv) = (
                    tex_of(&self.out_y)?.tex.clone(),
                    tex_of(&self.out_u)?.tex.clone(),
                    tex_of(&self.out_v)?.tex.clone(),
                );
                for (tex, c) in [ty, tu, tv].iter().zip(&copies) {
                    self.copy_out(enc, tex, c)?;
                }
            }
        }
        Ok((copies, offset))
    }

    fn ensure_readback(&mut self, size: u64) {
        if self.readback.as_ref().is_some_and(|b| b.size() >= size) {
            return;
        }
        self.readback = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sc_render readback"),
            size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        }));
    }

    fn copy_out(
        &self,
        enc: &mut wgpu::CommandEncoder,
        tex: &wgpu::Texture,
        c: &Plane,
    ) -> Result<()> {
        let buffer = self
            .readback
            .as_ref()
            .ok_or_else(|| Error::Other("sc_render: readback buffer missing".into()))?;
        enc.copy_texture_to_buffer(
            tex.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: c.offset,
                    bytes_per_row: Some(c.bpr),
                    rows_per_image: Some(c.rows),
                },
            },
            extent(tex.width(), tex.height()),
        );
        Ok(())
    }

    fn read(&self, copies: &[Plane], total: u64) -> Result<Vec<u8>> {
        let buffer = self
            .readback
            .as_ref()
            .ok_or_else(|| Error::Other("sc_render: readback buffer missing".into()))?;
        let slice = buffer.slice(..total);
        let (tx, rx) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| other("poll", e))?;
        rx.recv()
            .map_err(|e| other("map callback", e))?
            .map_err(|e| other("map readback", e))?;
        let packed: usize = copies.iter().map(|c| (c.row * c.rows) as usize).sum();
        let mut data = Vec::with_capacity(packed);
        {
            let mapped = slice
                .get_mapped_range()
                .map_err(|e| other("mapped range", e))?;
            for c in copies {
                for r in 0..c.rows as usize {
                    let s = c.offset as usize + r * c.bpr as usize;
                    data.extend_from_slice(&mapped[s..s + c.row as usize]);
                }
            }
        }
        buffer.unmap();
        debug_assert_eq!(data.len(), packed);
        Ok(data)
    }
}

struct Plane {
    offset: u64,
    bpr: u32,
    row: u32,
    rows: u32,
}

fn extent(w: u32, h: u32) -> wgpu::Extent3d {
    wgpu::Extent3d {
        width: w,
        height: h,
        depth_or_array_layers: 1,
    }
}
