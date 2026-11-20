//! wgpu compositor. Every shader stage mirrors the integer maths in `maths.rs`.

use std::sync::Arc;
use std::sync::mpsc;

use sc_geometry::SizePx;
use sc_platform::{Error, Nv12View, Result};

use crate::cpu::{check_job, check_view};
use crate::maths::{self, ScreenPlan};
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

const TARGET: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const ROW_ALIGN: u32 = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;

struct Tex {
    tex: wgpu::Texture,
    view: wgpu::TextureView,
}

pub(crate) struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    summary: AdapterSummary,
    max_dim: u32,
    prefilter: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    to_y: wgpu::RenderPipeline,
    to_uv: wgpu::RenderPipeline,
    pre_ubuf: wgpu::Buffer,
    comp_ubuf: wgpu::Buffer,
    frame_y: Option<Tex>,
    frame_uv: Option<Tex>,
    pre: Option<Tex>,
    target: Option<Tex>,
    out_y: Option<Tex>,
    out_u: Option<Tex>,
    out_v: Option<Tex>,
    readback: Option<wgpu::Buffer>,
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
        match Gpu::new(&adapter) {
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
    fn new(adapter: &wgpu::Adapter) -> Result<Gpu> {
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
        let ubuf = |label| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: 64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let pre_ubuf = ubuf("sc_render pre uniforms");
        let comp_ubuf = ubuf("sc_render comp uniforms");
        if let Some(e) = pollster::block_on(scope.pop()) {
            return Err(other("pipeline setup", e));
        }
        Ok(Gpu {
            summary: summarize(&adapter.get_info()),
            max_dim: limits.max_texture_dimension_2d,
            device,
            queue,
            prefilter,
            composite,
            to_y,
            to_uv,
            pre_ubuf,
            comp_ubuf,
            frame_y: None,
            frame_uv: None,
            pre: None,
            target: None,
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
        let canvas = job.canvas;
        if canvas.w > self.max_dim || canvas.h > self.max_dim {
            return Err(Error::Other(format!(
                "sc_render: canvas {}x{} exceeds the adapter's {} px texture limit",
                canvas.w, canvas.h, self.max_dim
            )));
        }
        let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let oom = self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let result = self.render_inner(job, out);
        let oom = pollster::block_on(oom.pop());
        let validation = pollster::block_on(scope.pop());
        if let Some(e) = validation.or(oom) {
            return Err(other("wgpu", e));
        }
        result
    }

    fn render_inner(&mut self, job: &FrameJob, out: OutputFormat) -> Result<RenderedFrame> {
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
        self.queue.write_texture(
            tex_of(&self.target)?.tex.as_image_copy(),
            &bg.data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bg.stride as u32),
                rows_per_image: Some(canvas.h),
            },
            extent(canvas.w, canvas.h),
        );

        let mut plan = None;
        if let Some(layer) = &job.screen {
            let mut result = Ok(None);
            layer.frame.0.with_nv12(&mut |view| {
                result = self.upload_frame(canvas, layer.src, layer.dst, &view);
            })?;
            plan = result?;
        }

        let mut enc = self.device.create_command_encoder(&Default::default());
        if let Some(plan) = &plan {
            self.encode_screen(&mut enc, plan)?;
        }
        let (copies, total) = self.encode_output(&mut enc, canvas, out)?;
        self.queue.submit([enc.finish()]);
        let data = self.read(&copies, total)?;
        Ok(RenderedFrame {
            size: canvas,
            format: out,
            data,
        })
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
        let (w, h) = (view.size.w, view.size.h);
        let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
        let usage = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST;
        ensure_tex(
            &self.device,
            &mut self.frame_y,
            "sc_render frame y",
            (w, h),
            wgpu::TextureFormat::R8Uint,
            usage,
        );
        ensure_tex(
            &self.device,
            &mut self.frame_uv,
            "sc_render frame uv",
            (cw, ch),
            wgpu::TextureFormat::Rg8Uint,
            usage,
        );
        self.queue.write_texture(
            tex_of(&self.frame_y)?.tex.as_image_copy(),
            &view.y[..view.y_stride * (h as usize - 1) + w as usize],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(view.y_stride as u32),
                rows_per_image: Some(h),
            },
            extent(w, h),
        );
        self.queue.write_texture(
            tex_of(&self.frame_uv)?.tex.as_image_copy(),
            &view.uv[..view.uv_stride * (ch as usize - 1) + 2 * cw as usize],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(view.uv_stride as u32),
                rows_per_image: Some(ch),
            },
            extent(cw, ch),
        );
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
