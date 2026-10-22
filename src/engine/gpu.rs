//! Thin wgpu layer: one shader module, full-screen passes, a texture pool and
//! a per-frame uniform arena with dynamic offsets.

use eframe::wgpu;
use std::collections::HashMap;

/// Working format for every canvas / layer texture (premultiplied, gamma-encoded).
pub const FMT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const SLOT: u64 = 768;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct U {
    pub canvas: [f32; 4],
    pub src: [f32; 4],
    pub m: [[f32; 4]; 6],
    pub cfg: [f32; 4],
    pub crop: [f32; 4],
    pub ga: [[f32; 4]; 12],
    pub gf: [[f32; 4]; 12],
    pub gmix: [f32; 4],
    pub mask: [[f32; 4]; 3],
    pub chroma: [[f32; 4]; 2],
    pub p: [[f32; 4]; 4],
    pub time: [f32; 4],
}

impl Default for U {
    fn default() -> Self {
        let mut u: U = bytemuck::Zeroable::zeroed();
        u.ga = super::color::identity_block();
        u.gf = super::color::identity_block();
        u.cfg = [1.0, 1.0, 0.0, 0.0];
        u.crop = [0.0, 0.0, 1.0, 1.0];
        u
    }
}

impl U {
    pub fn with_canvas(w: u32, h: u32) -> Self {
        let mut u = U::default();
        u.canvas = [w as f32, h as f32, 1.0 / w.max(1) as f32, 1.0 / h.max(1) as f32];
        u
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Pipe {
    Composite,
    Grade,
    Yuv,
    Nv12,
    Copy,
    Blur,
    Transition,
    Effect,
}

pub struct Tex {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub w: u32,
    pub h: u32,
    pub format: wgpu::TextureFormat,
}

pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    bgl_u: wgpu::BindGroupLayout,
    bgl_t: wgpu::BindGroupLayout,
    pipes: HashMap<Pipe, wgpu::RenderPipeline>,
    samp: wgpu::Sampler,
    samp_n: wgpu::Sampler,
    ubuf: wgpu::Buffer,
    ubind: wgpu::BindGroup,
    ucap: u64,
    uoff: u64,
    dummy: Tex,
    free: Vec<Tex>,
    pub max_dim: u32,
}

pub fn headless_device() -> anyhow::Result<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::from_env().unwrap_or(wgpu::Backends::PRIMARY | wgpu::Backends::GL),
        ..Default::default()
    });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .ok_or_else(|| anyhow::anyhow!("no GPU adapter found (Vulkan/GL)"))?;
    let lim = adapter.limits();
    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("velomov-headless"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits { max_texture_dimension_2d: lim.max_texture_dimension_2d.min(16384), ..wgpu::Limits::default() },
            memory_hints: wgpu::MemoryHints::Performance,
        },
        None,
    ))?;
    Ok((device, queue))
}

fn make_tex(device: &wgpu::Device, w: u32, h: u32, format: wgpu::TextureFormat, srgb_view: bool) -> Tex {
    let view_formats: &[wgpu::TextureFormat] = if srgb_view { &[wgpu::TextureFormat::Rgba8UnormSrgb] } else { &[] };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("velomov-tex"),
        size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
        view_formats,
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    Tex { texture, view, w: w.max(1), h: h.max(1), format }
}

impl Gpu {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("velomov-shaders"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders.wgsl").into()),
        });
        let bgl_u = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("velomov-u"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<U>() as u64),
                },
                count: None,
            }],
        });
        let tex_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let samp_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let bgl_t = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("velomov-t"),
            entries: &[tex_entry(0), tex_entry(1), tex_entry(2), samp_entry(3), samp_entry(4)],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("velomov-layout"),
            bind_group_layouts: &[&bgl_u, &bgl_t],
            push_constant_ranges: &[],
        });
        let mut pipes = HashMap::new();
        for (pipe, entry) in [
            (Pipe::Composite, "fs_composite"),
            (Pipe::Grade, "fs_grade"),
            (Pipe::Yuv, "fs_yuv"),
            (Pipe::Nv12, "fs_nv12"),
            (Pipe::Copy, "fs_copy"),
            (Pipe::Blur, "fs_blur"),
            (Pipe::Transition, "fs_transition"),
            (Pipe::Effect, "fs_effect"),
        ] {
            let p = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    targets: &[Some(wgpu::ColorTargetState { format: FMT, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            });
            pipes.insert(pipe, p);
        }
        let samp = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("lin"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let samp_n = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("near"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let ucap = SLOT * 64;
        let (ubuf, ubind) = Self::make_ubuf(&device, &bgl_u, ucap);
        let dummy = make_tex(&device, 2, 2, FMT, false);
        let max_dim = device.limits().max_texture_dimension_2d;
        Gpu { device, queue, bgl_u, bgl_t, pipes, samp, samp_n, ubuf, ubind, ucap, uoff: 0, dummy, free: vec![], max_dim }
    }

    fn make_ubuf(device: &wgpu::Device, bgl_u: &wgpu::BindGroupLayout, cap: u64) -> (wgpu::Buffer, wgpu::BindGroup) {
        let ubuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("velomov-uniforms"),
            size: cap,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let ubind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("velomov-u"),
            layout: bgl_u,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &ubuf,
                    offset: 0,
                    size: wgpu::BufferSize::new(std::mem::size_of::<U>() as u64),
                }),
            }],
        });
        (ubuf, ubind)
    }

    pub fn begin_frame(&mut self) {
        self.uoff = 0;
    }

    /// A pooled canvas-format texture.
    pub fn tex(&mut self, w: u32, h: u32) -> Tex {
        let (w, h) = (w.clamp(1, self.max_dim), h.clamp(1, self.max_dim));
        if let Some(i) = self.free.iter().position(|t| t.w == w && t.h == h && t.format == FMT) {
            return self.free.swap_remove(i);
        }
        make_tex(&self.device, w, h, FMT, false)
    }

    /// A texture with an extra sRGB view format (for egui display).
    pub fn display_tex(&self, w: u32, h: u32) -> Tex {
        make_tex(&self.device, w, h, FMT, true)
    }

    pub fn plane_tex(&self, w: u32, h: u32) -> Tex {
        make_tex(&self.device, w, h, wgpu::TextureFormat::R8Unorm, false)
    }

    /// Two-channel plane (interleaved chroma of NV12 frames).
    pub fn plane_tex_rg(&self, w: u32, h: u32) -> Tex {
        make_tex(&self.device, w, h, wgpu::TextureFormat::Rg8Unorm, false)
    }

    pub fn release(&mut self, t: Tex) {
        if t.format != FMT {
            return;
        }
        self.free.push(t);
        // keep the pool bounded
        while self.free.len() > 28 {
            self.free.remove(0);
        }
    }

    pub fn trim_pool(&mut self) {
        self.free.clear();
    }

    pub fn pass(
        &mut self,
        enc: &mut wgpu::CommandEncoder,
        pipe: Pipe,
        target: &wgpu::TextureView,
        inputs: [Option<&wgpu::TextureView>; 3],
        u: &U,
    ) {
        if self.uoff + SLOT > self.ucap {
            self.ucap *= 2;
            let (b, g) = Self::make_ubuf(&self.device, &self.bgl_u, self.ucap);
            self.ubuf = b;
            self.ubind = g;
            self.uoff = 0;
        }
        let off = self.uoff;
        self.uoff += SLOT;
        self.queue.write_buffer(&self.ubuf, off, bytemuck::bytes_of(u));
        let d = &self.dummy.view;
        let v0 = inputs[0].unwrap_or(d);
        let v1 = inputs[1].unwrap_or(d);
        let v2 = inputs[2].unwrap_or(d);
        let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.bgl_t,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(v0) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(v1) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(v2) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.samp) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&self.samp_n) },
            ],
        });
        let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        rp.set_pipeline(&self.pipes[&pipe]);
        rp.set_bind_group(0, &self.ubind, &[off as u32]);
        rp.set_bind_group(1, &bg, &[]);
        rp.draw(0..3, 0..1);
    }

    pub fn clear(&self, enc: &mut wgpu::CommandEncoder, target: &wgpu::TextureView, rgba: [f32; 4]) {
        let _rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color { r: rgba[0] as f64, g: rgba[1] as f64, b: rgba[2] as f64, a: rgba[3] as f64 }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
    }

    pub fn upload(&self, tex: &Tex, data: &[u8], bpp: u32) {
        let need = (tex.w * tex.h * bpp) as usize;
        if data.len() < need {
            return;
        }
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &tex.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &data[..need],
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(tex.w * bpp), rows_per_image: Some(tex.h) },
            wgpu::Extent3d { width: tex.w, height: tex.h, depth_or_array_layers: 1 },
        );
    }

    /// Blocking readback of an Rgba8 texture (tightly packed rows).
    pub fn read_rgba(&self, tex: &Tex) -> Vec<u8> {
        let stride = (tex.w * 4 + 255) / 256 * 256;
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: (stride * tex.h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("readback") });
        enc.copy_texture_to_buffer(
            tex.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(stride), rows_per_image: Some(tex.h) },
            },
            wgpu::Extent3d { width: tex.w, height: tex.h, depth_or_array_layers: 1 },
        );
        self.queue.submit(Some(enc.finish()));
        let slice = buf.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        let _ = self.device.poll(wgpu::Maintain::Wait);
        let _ = rx.recv();
        let data = slice.get_mapped_range();
        let mut out = Vec::with_capacity((tex.w * tex.h * 4) as usize);
        for y in 0..tex.h {
            let s = (y * stride) as usize;
            out.extend_from_slice(&data[s..s + (tex.w * 4) as usize]);
        }
        drop(data);
        buf.unmap();
        out
    }
}

/// Pipelined readback ring used by the exporter.
pub struct Readback {
    pub w: u32,
    pub h: u32,
    stride: u32,
    bufs: Vec<(wgpu::Buffer, Option<u64>)>,
    next: usize,
}

impl Readback {
    pub fn new(device: &wgpu::Device, w: u32, h: u32, depth: usize) -> Self {
        let stride = (w * 4 + 255) / 256 * 256;
        let bufs = (0..depth)
            .map(|_| {
                (
                    device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("export-readback"),
                        size: (stride * h) as u64,
                        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                        mapped_at_creation: false,
                    }),
                    None,
                )
            })
            .collect();
        Readback { w, h, stride, bufs, next: 0 }
    }

    /// Slot that will be written next; if it still holds a frame, that frame must be collected first.
    pub fn pending_in_next(&self) -> Option<u64> {
        self.bufs[self.next].1
    }

    pub fn copy(&mut self, enc: &mut wgpu::CommandEncoder, tex: &Tex, frame: u64) {
        let (buf, slot) = &mut self.bufs[self.next];
        enc.copy_texture_to_buffer(
            tex.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: buf,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(self.stride), rows_per_image: Some(self.h) },
            },
            wgpu::Extent3d { width: self.w, height: self.h, depth_or_array_layers: 1 },
        );
        *slot = Some(frame);
        self.next = (self.next + 1) % self.bufs.len();
    }

    /// Wait for the oldest outstanding frame and hand its rows to `sink`.
    pub fn collect_oldest(&mut self, device: &wgpu::Device, sink: &mut dyn FnMut(u64, &[u8])) -> bool {
        // oldest = the slot at `next` if occupied, otherwise scan forward
        let n = self.bufs.len();
        let mut idx = None;
        let mut best = u64::MAX;
        for i in 0..n {
            if let Some(f) = self.bufs[i].1 {
                if f < best {
                    best = f;
                    idx = Some(i);
                }
            }
        }
        let Some(i) = idx else { return false };
        let (buf, slot) = &mut self.bufs[i];
        let frame = slot.take().unwrap();
        let slice = buf.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        let _ = device.poll(wgpu::Maintain::Wait);
        let _ = rx.recv();
        {
            let data = slice.get_mapped_range();
            let row = (self.w * 4) as usize;
            if self.stride as usize == row {
                sink(frame, &data[..row * self.h as usize]);
            } else {
                let mut packed = Vec::with_capacity(row * self.h as usize);
                for y in 0..self.h as usize {
                    let s = y * self.stride as usize;
                    packed.extend_from_slice(&data[s..s + row]);
                }
                sink(frame, &packed);
            }
        }
        buf.unmap();
        true
    }
}
