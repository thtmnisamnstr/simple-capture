use std::time::Instant;

use crate::PxRect;

pub const BG: [f64; 3] = [24.0 / 255.0, 24.0 / 255.0, 28.0 / 255.0];

pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    ubuf: wgpu::Buffer,
    bind: wgpu::BindGroup,
    start: Instant,
    /// Some(rect): the surface covers the whole window and the preview is drawn
    /// only inside `rect` (parent mode). None: the surface is the preview.
    pub viewport: Option<PxRect>,
    pub adapter_desc: String,
}

impl Renderer {
    pub fn new(instance: &wgpu::Instance, surface: wgpu::Surface<'static>, w: u32, h: u32) -> Self {
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .expect("no wgpu adapter for this surface");
        let info = adapter.get_info();
        let adapter_desc = format!("{:?} / {} / {:?}", info.backend, info.name, info.device_type);
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("preview"),
            ..Default::default()
        }))
        .expect("no wgpu device");

        let caps = surface.get_capabilities(&adapter);
        let format = if caps.formats.contains(&wgpu::TextureFormat::Bgra8Unorm) {
            wgpu::TextureFormat::Bgra8Unorm
        } else {
            caps.formats[0]
        };
        let alpha_mode = if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::Opaque) {
            wgpu::CompositeAlphaMode::Opaque
        } else {
            caps.alpha_modes[0]
        };
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: w.max(1),
            height: h.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode,
            view_formats: vec![],
            color_space: wgpu::SurfaceColorSpace::Auto,
        };
        surface.configure(&device, &config);

        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("pattern"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("pattern"),
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
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(format.into())],
            }),
            multiview_mask: None,
            cache: None,
        });
        let ubuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uniforms"),
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("uniforms"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() }],
        });

        Self {
            device,
            queue,
            surface,
            config,
            pipeline,
            ubuf,
            bind,
            start: Instant::now(),
            viewport: None,
            adapter_desc,
        }
    }

    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    pub fn resize(&mut self, w: u32, h: u32) {
        let (w, h) = (w.max(1), h.max(1));
        if (w, h) != self.size() {
            self.config.width = w;
            self.config.height = h;
            self.surface.configure(&self.device, &self.config);
        }
    }

    /// Renders and presents one frame. Returns false when no frame was presented.
    pub fn render(&mut self) -> bool {
        let tex = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return false;
            }
            _ => return false,
        };
        let (w, h) = self.size();
        let vp = match self.viewport {
            Some(r) => {
                let x = r.x.clamp(0, w as i32) as u32;
                let y = r.y.clamp(0, h as i32) as u32;
                let rw = r.w.min(w - x);
                let rh = r.h.min(h - y);
                (x, y, rw, rh)
            }
            None => (0, 0, w, h),
        };
        let t = self.start.elapsed().as_secs_f32();
        let u = [t, vp.0 as f32, vp.1 as f32, vp.2.max(1) as f32, vp.3.max(1) as f32, 0.0, 0.0, 0.0];
        let mut bytes = [0u8; 32];
        for (i, v) in u.iter().enumerate() {
            bytes[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }
        self.queue.write_buffer(&self.ubuf, 0, &bytes);

        let view = tex.texture.create_view(&Default::default());
        let mut enc = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("preview"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: BG[0], g: BG[1], b: BG[2], a: 1.0 }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if vp.2 > 0 && vp.3 > 0 {
                pass.set_viewport(vp.0 as f32, vp.1 as f32, vp.2 as f32, vp.3 as f32, 0.0, 1.0);
                pass.set_scissor_rect(vp.0, vp.1, vp.2, vp.3);
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &self.bind, &[]);
                pass.draw(0..3, 0..1);
            }
        }
        self.queue.submit(Some(enc.finish()));
        self.queue.present(tex);
        true
    }
}
