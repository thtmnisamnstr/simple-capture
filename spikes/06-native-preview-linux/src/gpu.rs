use std::time::Instant;

pub const SRC_W: u32 = 1920;
pub const SRC_H: u32 = 1080;
pub const HEADER_LEN: usize = 32;
pub const MAGIC: u32 = 0x5643_5053; // "SPCV" little-endian

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    frame_id: u32,
    ts_lo: u32,
    t: f32,
    _pad: f32,
}

pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub adapter: wgpu::Adapter,
    pub adapter_info: wgpu::AdapterInfo,
    composite_pipeline: wgpu::RenderPipeline,
    composite_bg: wgpu::BindGroup,
    ubuf: wgpu::Buffer,
    comp_view: wgpu::TextureView,
    blit_module: wgpu::ShaderModule,
    sampler: wgpu::Sampler,
    blit_pipelines: Vec<(wgpu::TextureFormat, wgpu::RenderPipeline, wgpu::BindGroup)>,
    preview: Option<Readback>,
}

struct Readback {
    w: u32,
    h: u32,
    view: wgpu::TextureView,
    tex: wgpu::Texture,
    buf: wgpu::Buffer,
}

#[derive(Default, Clone, Copy)]
pub struct FrameTimings {
    pub encode_ms: f64,
    pub gpu_wait_ms: f64,
    pub copy_ms: f64,
}

impl Gpu {
    pub fn new(instance: &wgpu::Instance, surface: Option<&wgpu::Surface<'_>>) -> Self {
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: surface,
            ..Default::default()
        }))
        .expect("no wgpu adapter");
        let adapter_info = adapter.get_info();
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("spike"),
            ..Default::default()
        }))
        .expect("no wgpu device");

        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        let comp_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("composite"),
            size: wgpu::Extent3d { width: SRC_W, height: SRC_H, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let comp_view = comp_tex.create_view(&Default::default());

        let composite_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("composite"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_composite"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::TextureFormat::Rgba8Unorm.into())],
            }),
            multiview_mask: None,
            cache: None,
        });
        let ubuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let composite_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("composite_bg"),
            layout: &composite_pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() }],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("linear"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        Self {
            device,
            queue,
            adapter,
            adapter_info,
            composite_pipeline,
            composite_bg,
            ubuf,
            comp_view,
            blit_module: module,
            sampler,
            blit_pipelines: Vec::new(),
            preview: None,
        }
    }

    fn blit_for(&mut self, format: wgpu::TextureFormat) -> usize {
        if let Some(i) = self.blit_pipelines.iter().position(|(f, _, _)| *f == format) {
            return i;
        }
        let p = self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("blit"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &self.blit_module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &self.blit_module,
                entry_point: Some("fs_blit"),
                compilation_options: Default::default(),
                targets: &[Some(format.into())],
            }),
            multiview_mask: None,
            cache: None,
        });
        let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("blit_bg"),
            layout: &p.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&self.comp_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });
        self.blit_pipelines.push((format, p, bg));
        self.blit_pipelines.len() - 1
    }

    pub fn set_readback_size(&mut self, w: u32, h: u32) {
        assert!((w * 4) % wgpu::COPY_BYTES_PER_ROW_ALIGNMENT == 0, "row pitch must be 256-aligned");
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("preview"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = tex.create_view(&Default::default());
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: (w * h * 4) as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.blit_for(wgpu::TextureFormat::Rgba8Unorm);
        self.preview = Some(Readback { w, h, view, tex, buf });
    }

    fn encode_composite(&mut self, enc: &mut wgpu::CommandEncoder, frame_id: u32, ts_ms: f64, t: f32) {
        let un = Uniforms { frame_id, ts_lo: (ts_ms as u64 & 0xffff_ffff) as u32, t, _pad: 0.0 };
        self.queue.write_buffer(&self.ubuf, 0, bytemuck::bytes_of(&un));
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("composite"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.comp_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.composite_pipeline);
        pass.set_bind_group(0, &self.composite_bg, &[]);
        pass.draw(0..3, 0..1);
    }

    fn encode_blit(&mut self, enc: &mut wgpu::CommandEncoder, target: &wgpu::TextureView, format: wgpu::TextureFormat) {
        let i = self.blit_for(format);
        let (_, p, bg) = &self.blit_pipelines[i];
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("blit"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(p);
        pass.set_bind_group(0, bg, &[]);
        pass.draw(0..3, 0..1);
    }

    /// Composite at 1920x1080, GPU-downscale to the readback size, read it back
    /// into a fresh Vec with a 32-byte header in front of tightly packed RGBA8.
    pub fn render_readback(&mut self, frame_id: u32, ts_ms: f64, t: f32) -> (Vec<u8>, FrameTimings) {
        let t0 = Instant::now();
        let (w, h) = {
            let p = self.preview.as_ref().expect("readback size not set");
            (p.w, p.h)
        };
        let mut enc = self.device.create_command_encoder(&Default::default());
        self.encode_composite(&mut enc, frame_id, ts_ms, t);
        let pv = self.preview.as_ref().unwrap().view.clone();
        self.encode_blit(&mut enc, &pv, wgpu::TextureFormat::Rgba8Unorm);
        let p = self.preview.as_ref().unwrap();
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &p.tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &p.buf,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(h) },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        self.queue.submit([enc.finish()]);
        let slice = p.buf.slice(..);
        slice.map_async(wgpu::MapMode::Read, |r| r.expect("map failed"));
        let t1 = Instant::now();
        self.device.poll(wgpu::PollType::wait_indefinitely()).expect("poll");
        let t2 = Instant::now();
        let n = (w * h * 4) as usize;
        let mut out = Vec::with_capacity(HEADER_LEN + n);
        out.extend_from_slice(&MAGIC.to_le_bytes());
        out.extend_from_slice(&frame_id.to_le_bytes());
        out.extend_from_slice(&w.to_le_bytes());
        out.extend_from_slice(&h.to_le_bytes());
        out.extend_from_slice(&ts_ms.to_le_bytes());
        out.extend_from_slice(&0f64.to_le_bytes());
        {
            let data = slice.get_mapped_range().expect("mapped range");
            out.extend_from_slice(&data);
        }
        p.buf.unmap();
        let t3 = Instant::now();
        (
            out,
            FrameTimings {
                encode_ms: (t1 - t0).as_secs_f64() * 1e3,
                gpu_wait_ms: (t2 - t1).as_secs_f64() * 1e3,
                copy_ms: (t3 - t2).as_secs_f64() * 1e3,
            },
        )
    }

    /// Composite at 1920x1080 and scale straight into a surface texture.
    pub fn render_to_view(&mut self, view: &wgpu::TextureView, format: wgpu::TextureFormat, frame_id: u32, ts_ms: f64, t: f32) {
        let mut enc = self.device.create_command_encoder(&Default::default());
        self.encode_composite(&mut enc, frame_id, ts_ms, t);
        self.encode_blit(&mut enc, view, format);
        self.queue.submit([enc.finish()]);
    }
}

pub fn stamp_send_time(buf: &mut [u8], ms: f64) {
    buf[24..32].copy_from_slice(&ms.to_le_bytes());
}
