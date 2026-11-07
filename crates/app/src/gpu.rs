use crate::composite::FrameImage;
use crate::stats::{Stats, lock};
use egui_wgpu::wgpu;
use half::f16;
use klad_brush::TILE;
use rayon::prelude::*;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub const MIP_LEVELS: u32 = 9;

struct Plane {
    texture: wgpu::Texture,
    uniform: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    size: (u32, u32),
    levels: u32,
}

pub fn levels_for((width, height): (u32, u32)) -> u32 {
    (32 - width.max(height).max(1).leading_zeros()).min(MIP_LEVELS)
}

pub fn mip_chain(tile: &[f16]) -> Vec<Vec<f16>> {
    let mut levels: Vec<Vec<f16>> = Vec::new();
    let mut side = TILE;
    let mut current: Vec<f32> = tile.iter().map(|v| v.to_f32()).collect();
    while side > 1 && levels.len() + 1 < MIP_LEVELS as usize {
        let half = side / 2;
        let next: Vec<f32> = (0..half * half * 4)
            .map(|i| {
                let (c, px) = (i % 4, i / 4);
                let (x, y) = (px % half * 2, px / half * 2);
                let at = |x: usize, y: usize| current[(y * side + x) * 4 + c];
                (at(x, y) + at(x + 1, y) + at(x, y + 1) + at(x + 1, y + 1)) * 0.25
            })
            .collect();
        levels.push(next.iter().map(|v| f16::from_f32(*v)).collect());
        current = next;
        side = half;
    }
    levels
}

pub struct CanvasGpu {
    canvas_pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    canvas: Plane,
    frames: BTreeMap<u64, Plane>,
}

fn texture(device: &wgpu::Device, (width, height): (u32, u32), levels: u32) -> wgpu::Texture {
    texture_as(device, (width, height), levels, wgpu::TextureFormat::Rgba16Float)
}

fn texture_as(
    device: &wgpu::Device,
    (width, height): (u32, u32),
    levels: u32,
    format: wgpu::TextureFormat,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("plane"),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: levels,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

fn plane(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    size: (u32, u32),
    uniform_bytes: u64,
    levels: u32,
) -> Plane {
    let uniform = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("plane uniform"),
        size: uniform_bytes,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let texture = texture(device, size, levels);
    let bind_group = bind(device, layout, &uniform, &texture, sampler);
    Plane {
        texture,
        uniform,
        bind_group,
        size,
        levels,
    }
}

fn bind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniform: &wgpu::Buffer,
    texture: &wgpu::Texture,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("plane"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&texture.create_view(&Default::default())),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

fn pipeline(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
    source: &str,
    blend: Option<wgpu::BlendState>,
) -> wgpu::RenderPipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("plane"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("plane"),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("plane"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        multiview_mask: None,
        cache: None,
    })
}

impl CanvasGpu {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat, size: (u32, u32)) -> Self {
        let entry = |binding, visibility, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty,
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("plane"),
            entries: &[
                entry(
                    0,
                    wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
                entry(
                    1,
                    wgpu::ShaderStages::FRAGMENT,
                    wgpu::BindingType::Texture {
                        multisampled: false,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                ),
                entry(
                    2,
                    wgpu::ShaderStages::FRAGMENT,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                ),
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("plane"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        Self {
            canvas_pipeline: pipeline(device, &layout, format, include_str!("canvas.wgsl"), None),
            canvas: plane(device, &layout, &sampler, size, 48, levels_for(size)),
            frames: BTreeMap::new(),
            layout,
            sampler,
        }
    }

    fn put_frame(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, image: &FrameImage) {
        let size = (image.width, image.height);
        let levels = image.mips.len() as u32;
        let texture = texture_as(device, size, levels, wgpu::TextureFormat::Rgba8Unorm);
        image.mips.iter().enumerate().for_each(|(level, pixels)| {
            let (w, h) = ((size.0 >> level).max(1), (size.1 >> level).max(1));
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: level as u32,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(w * 4),
                    rows_per_image: None,
                },
                wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
            );
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frame uniform"),
            size: 48,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = bind(device, &self.layout, &uniform, &texture, &self.sampler);
        self.frames.insert(
            image.key,
            Plane {
                texture,
                uniform,
                bind_group,
                size,
                levels,
            },
        );
    }

    fn resize(&mut self, device: &wgpu::Device, size: (u32, u32)) {
        let levels = levels_for(size);
        let target = &mut self.canvas;
        target.texture = texture(device, size, levels);
        target.bind_group = bind(device, &self.layout, &target.uniform, &target.texture, &self.sampler);
        target.size = size;
        target.levels = levels;
    }
}

type Upload = (usize, Vec<f16>);

fn upload_level(queue: &wgpu::Queue, plane: &Plane, level: u32, (x, y): (u32, u32), side: u32, pixels: &[f16]) {
    let (w, h) = ((plane.size.0 >> level).max(1), (plane.size.1 >> level).max(1));
    if x >= w || y >= h {
        return;
    }
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &plane.texture,
            mip_level: level,
            origin: wgpu::Origin3d { x, y, z: 0 },
            aspect: wgpu::TextureAspect::All,
        },
        bytemuck::cast_slice(pixels),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(side * 8),
            rows_per_image: None,
        },
        wgpu::Extent3d {
            width: side.min(w - x),
            height: side.min(h - y),
            depth_or_array_layers: 1,
        },
    );
}

fn upload_tiles(queue: &wgpu::Queue, plane: &Plane, cols: usize, tiles: &[Upload]) {
    let chains: Vec<Vec<Vec<f16>>> = if plane.levels > 1 {
        tiles.par_iter().map(|(_, tile)| mip_chain(tile)).collect()
    } else {
        tiles.iter().map(|_| Vec::new()).collect()
    };
    tiles.iter().zip(&chains).for_each(|((index, tile), chain)| {
        let (tx, ty) = ((index % cols) as u32, (index / cols) as u32);
        upload_level(queue, plane, 0, (tx * TILE as u32, ty * TILE as u32), TILE as u32, tile);
        chain
            .iter()
            .enumerate()
            .take(plane.levels as usize - 1)
            .for_each(|(k, pixels)| {
                let level = k as u32 + 1;
                let side = TILE as u32 >> level;
                upload_level(queue, plane, level, (tx * side, ty * side), side, pixels);
            });
    });
}

pub struct CanvasPaint {
    pub uniform: [f32; 12],
    pub uploads: Mutex<Vec<(usize, Vec<f16>)>>,
    pub size: (u32, u32),
    pub cols: usize,
    pub pending: Option<Instant>,
    pub stats: Arc<Mutex<Stats>>,
    pub show: Option<u64>,
    pub frame_uploads: Mutex<Vec<FrameImage>>,
    pub frame_drops: Vec<u64>,
}

impl egui_wgpu::CallbackTrait for CanvasPaint {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Some(gpu) = resources.get_mut::<CanvasGpu>() else {
            return Vec::new();
        };
        if gpu.canvas.size != self.size {
            gpu.resize(device, self.size);
        }
        queue.write_buffer(&gpu.canvas.uniform, 0, bytemuck::cast_slice(&self.uniform));
        let uploads = std::mem::take(&mut *lock(&self.uploads));
        let start = Instant::now();
        upload_tiles(queue, &gpu.canvas, self.cols, &uploads);
        self.frame_drops.iter().for_each(|key| {
            gpu.frames.remove(key);
        });
        let images = std::mem::take(&mut *lock(&self.frame_uploads));
        let started = Instant::now();
        images.iter().for_each(|image| gpu.put_frame(device, queue, image));
        if let Some(plane) = self.show.and_then(|key| gpu.frames.get(&key)) {
            queue.write_buffer(&plane.uniform, 0, bytemuck::cast_slice(&self.uniform));
        }
        let mut stats = lock(&self.stats);
        if !images.is_empty() {
            stats.frame_upload_ms.push(started.elapsed().as_secs_f32() * 1000.0);
        }
        if !uploads.is_empty() {
            stats.upload_ms.push(start.elapsed().as_secs_f32() * 1000.0);
        }
        if let Some(pending) = self.pending {
            stats.latency_ms.push(pending.elapsed().as_secs_f32() * 1000.0);
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        let Some(gpu) = resources.get::<CanvasGpu>() else {
            return;
        };
        pass.set_pipeline(&gpu.canvas_pipeline);
        let under = self.show.and_then(|key| gpu.frames.get(&key)).unwrap_or(&gpu.canvas);
        pass.set_bind_group(0, &under.bind_group, &[]);
        pass.draw(0..6, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mip_chains_average_down_to_one_pixel() {
        let tile: Vec<f16> = (0..TILE * TILE)
            .flat_map(|i| {
                let (x, y) = (i % TILE, i / TILE);
                let v = if (x + y) % 2 == 0 { 1.0 } else { 0.0 };
                [v, v * 0.5, 0.25, 1.0].map(f16::from_f32)
            })
            .collect();
        let chain = mip_chain(&tile);
        assert_eq!(chain.len(), 8);
        assert_eq!(
            chain.iter().map(|l| l.len() / 4).collect::<Vec<_>>(),
            [16384, 4096, 1024, 256, 64, 16, 4, 1]
        );
        assert!(
            chain[0]
                .chunks(4)
                .all(|px| px[0].to_f32() == 0.5 && px[1].to_f32() == 0.25)
        );
        assert_eq!(
            chain[7].iter().map(|v| v.to_f32()).collect::<Vec<_>>(),
            [0.5, 0.25, 0.25, 1.0]
        );
        assert_eq!(
            (levels_for((4096, 2048)), levels_for((256, 256)), levels_for((300, 10))),
            (9, 9, 9)
        );
        assert_eq!((levels_for((200, 100)), levels_for((1, 1))), (8, 1));
    }
}
