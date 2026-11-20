use crate::composite::{Draw, Image, quad};
use bytemuck::{Pod, Zeroable};

const SHADER: &str = r#"
struct Globals { size: vec2<f32>, pad: vec2<f32> };
@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var t: texture_2d<f32>;
@group(1) @binding(1) var s: sampler;

struct VOut {
  @builtin(position) pos: vec4<f32>,
  @location(0) uv: vec2<f32>,
  @location(1) op: f32,
};

@vertex
fn vs(@location(0) p: vec2<f32>, @location(1) uv: vec2<f32>, @location(2) op: f32) -> VOut {
  var o: VOut;
  let n = p / g.size * 2.0 - 1.0;
  o.pos = vec4<f32>(n.x, -n.y, 0.0, 1.0);
  o.uv = uv;
  o.op = op;
  return o;
}

@fragment
fn fs(i: VOut) -> @location(0) vec4<f32> {
  return textureSample(t, s, i.uv) * i.op;
}
"#;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    p: [f32; 2],
    uv: [f32; 2],
    op: f32,
}

pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub adapter_info: String,
    pipeline: wgpu::RenderPipeline,
    tex_bgl: wgpu::BindGroupLayout,
    glob_bgl: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

pub struct GpuTex {
    pub tex: wgpu::Texture,
    pub bind: wgpu::BindGroup,
    pub w: u32,
    pub h: u32,
}

pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

impl Gpu {
    pub fn new() -> Gpu {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .expect("adapter");
        let info = adapter.get_info();
        let adapter_info = format!("{} ({:?})", info.name, info.backend);
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).expect("device");
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("composite"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let glob_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let tex_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&glob_bgl), Some(&tex_bgl)],
            ..Default::default()
        });
        let blend = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("composite"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32],
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: FORMAT,
                    blend: Some(wgpu::BlendState { color: blend, alpha: blend }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Gpu { device, queue, adapter_info, pipeline, tex_bgl, glob_bgl, sampler }
    }

    pub fn wait(&self) {
        self.device.poll(wgpu::PollType::wait_indefinitely()).expect("poll");
    }

    pub fn create_tex(&self, w: u32, h: u32, usage: wgpu::TextureUsages) -> wgpu::Texture {
        self.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage,
            view_formats: &[],
        })
    }

    pub fn upload(&self, img: &Image) -> GpuTex {
        let tex = self.create_tex(
            img.w,
            img.h,
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC,
        );
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &img.px,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(img.w * 4), rows_per_image: Some(img.h) },
            wgpu::Extent3d { width: img.w, height: img.h, depth_or_array_layers: 1 },
        );
        let view = tex.create_view(&Default::default());
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.tex_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });
        GpuTex { tex, bind, w: img.w, h: img.h }
    }

    pub fn target(&self, w: u32, h: u32) -> wgpu::Texture {
        self.create_tex(
            w,
            h,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
        )
    }

    pub fn composite(&self, bg: &GpuTex, target: &wgpu::Texture, textures: &[GpuTex], draws: &[Draw]) {
        let (w, h) = (bg.w, bg.h);
        let mut verts: Vec<Vertex> = Vec::with_capacity(draws.len() * 6);
        for d in draws {
            let t = &textures[d.tex];
            let q = quad(d);
            let [l, tp, r, b] = d.src;
            let uv = [
                [l / t.w as f32, tp / t.h as f32],
                [r / t.w as f32, tp / t.h as f32],
                [r / t.w as f32, b / t.h as f32],
                [l / t.w as f32, b / t.h as f32],
            ];
            for i in [0usize, 1, 2, 0, 2, 3] {
                verts.push(Vertex { p: [q[i].0, q[i].1], uv: uv[i], op: d.opacity });
            }
        }
        let vbuf = wgpu::util::DeviceExt::create_buffer_init(
            &self.device,
            &wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(&verts),
                usage: wgpu::BufferUsages::VERTEX,
            },
        );
        let ubuf = wgpu::util::DeviceExt::create_buffer_init(
            &self.device,
            &wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(&[w as f32, h as f32, 0.0, 0.0]),
                usage: wgpu::BufferUsages::UNIFORM,
            },
        );
        let glob = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.glob_bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() }],
        });
        let mut enc = self.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_texture(
            bg.tex.as_image_copy(),
            target.as_image_copy(),
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        {
            let view = target.create_view(&Default::default());
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &glob, &[]);
            pass.set_vertex_buffer(0, vbuf.slice(..));
            for (i, d) in draws.iter().enumerate() {
                pass.set_bind_group(1, &textures[d.tex].bind, &[]);
                let s = (i * 6) as u32;
                pass.draw(s..s + 6, 0..1);
            }
        }
        self.queue.submit([enc.finish()]);
    }

    pub fn readback(&self, target: &wgpu::Texture) -> Image {
        let (w, h) = (target.width(), target.height());
        let bpr = (w * 4).div_ceil(256) * 256;
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (bpr * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = self.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            target.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(bpr), rows_per_image: Some(h) },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        self.queue.submit([enc.finish()]);
        let slice = buf.slice(..);
        slice.map_async(wgpu::MapMode::Read, |r| r.expect("map"));
        self.wait();
        let data = slice.get_mapped_range().expect("mapped");
        let mut px = vec![0u8; (w * h * 4) as usize];
        for y in 0..h as usize {
            let src = &data[y * bpr as usize..y * bpr as usize + w as usize * 4];
            px[y * w as usize * 4..(y + 1) * w as usize * 4].copy_from_slice(src);
        }
        drop(data);
        buf.unmap();
        Image { w, h, px }
    }
}
