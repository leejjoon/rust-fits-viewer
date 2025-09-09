mod raw_header;
mod parse;
mod upload;

use clap::Parser;
use eframe::egui;
use std::sync::Arc;
use wgpu::util::DeviceExt;
use fits_view_client::{
    MetaResponse, Viewport,
    RenderSize, ImageSize,
    create_image_to_ndc_matrix, calculate_max_lod, calculate_visible_tiles,
    tile_manager::{TileCoord},
};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(long, default_value = "http://127.0.0.1:8001")]
    backend_url: String,
    #[arg(long, default_value = "0.0")]
    initial_pan_x: f32,
    #[arg(long, default_value = "0.0")]
    initial_pan_y: f32,
    #[arg(long, default_value = "1.0")]
    initial_zoom: f32,
    #[arg(long, default_value = "800")]
    window_width: u32,
    #[arg(long, default_value = "600")]
    window_height: u32,
}

#[derive(Debug, Clone, Copy)]
struct SimpleViewport {
    pub zoom: f32,
    pub center_on_image: egui::Vec2, // Image pixel coordinates at the center of the view
    pub rotation_angle: f32,
}

impl Default for SimpleViewport {
    fn default() -> Self {
        let image_size = egui::Vec2::new(1024.0, 1024.0); // Default image size
        Self {
            zoom: 1.0,
            center_on_image: image_size * 0.5,
            rotation_angle: 0.0,
        }
    }
}

impl SimpleViewport {
    pub fn fit_to_window(&mut self, window_size: egui::Vec2, image_size: egui::Vec2) {
        let scale_x = window_size.x / image_size.x;
        let scale_y = window_size.y / image_size.y;
        self.zoom = scale_x.min(scale_y);
        self.center_on_image = image_size * 0.5;
        self.rotation_angle = 0.0;
    }

    pub fn set_center_on_image(&mut self, center: egui::Vec2) {
        self.center_on_image = center;
    }

    pub fn set_zoom(&mut self, zoom: f32) {
        self.zoom = zoom;
    }

    pub fn reset(&mut self, image_size: egui::Vec2) {
        self.zoom = 1.0;
        self.center_on_image = image_size * 0.5;
        self.rotation_angle = 0.0;
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 2],
    tex_coords: [f32; 2],
}

impl Vertex {
    fn desc<'a>() -> wgpu::VertexBufferLayout<'a> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 2]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x2,
                },
            ],
        }
    }
}

const VERTICES: &[Vertex] = &[
    Vertex { position: [0.0, 0.0], tex_coords: [0.0, 1.0] }, // Bottom-left
    Vertex { position: [1.0, 0.0], tex_coords: [1.0, 1.0] }, // Bottom-right
    Vertex { position: [1.0, 1.0], tex_coords: [1.0, 0.0] }, // Top-right
    Vertex { position: [0.0, 1.0], tex_coords: [0.0, 0.0] }, // Top-left
];

const INDICES: &[u16] = &[0, 1, 2, 0, 2, 3];

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    transform: [[f32; 4]; 4],
    tile_offset: [f32; 2],
    vmin: f32,
    vmax: f32,
    tile_size: f32,
    _padding1: f32,
    _padding2: [f32; 2],
    _padding3: [f32; 4],
}

struct TileData {
    _texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    _coord: TileCoord,
}

struct GpuTileManager {
    tiles: std::collections::HashMap<TileCoord, TileData>,
    tile_size: u32,
}

struct Custom3dResources {
    pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    num_indices: u32,
    uniform_bind_group: wgpu::BindGroup,
    uniform_buffer: wgpu::Buffer,
    tile_manager: GpuTileManager,
    texture_bind_group_layout: wgpu::BindGroupLayout,
}

struct Custom3d;

impl GpuTileManager {
    fn new(tile_size: u32) -> Self {
        Self { tiles: std::collections::HashMap::new(), tile_size }
    }

    fn get_or_create_tile(&mut self, coord: &TileCoord, device: &wgpu::Device, queue: &wgpu::Queue, backend_url: &str, texture_bind_group_layout: &wgpu::BindGroupLayout) -> Option<&TileData> {
        if self.tiles.contains_key(coord) {
            return self.tiles.get(coord);
        }

        let f32_data = match fetch_tile_data(backend_url, coord.lod, coord.x, coord.y) {
            Ok(data) => data,
            Err(e) => {
                println!("Failed to fetch tile ({}, {}, {}): {}, using fallback", coord.lod, coord.x, coord.y, e);
                self.generate_fallback_tile()
            }
        };

        let texture_size = wgpu::Extent3d { width: self.tile_size, height: self.tile_size, depth_or_array_layers: 1 };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("server_tile"),
            size: texture_size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        queue.write_texture(
            wgpu::ImageCopyTexture { texture: &texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            bytemuck::cast_slice(&f32_data),
            wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(4 * self.tile_size), rows_per_image: Some(self.tile_size) },
            texture_size,
        );

        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor { mag_filter: wgpu::FilterMode::Nearest, min_filter: wgpu::FilterMode::Nearest, ..Default::default() });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: texture_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&texture_view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
            ],
            label: Some(&format!("tile_{}_{}_{}", coord.lod, coord.x, coord.y)),
        });

        let tile_data = TileData { _texture: texture, bind_group, _coord: coord.clone() };
        self.tiles.insert(coord.clone(), tile_data);
        self.tiles.get(coord)
    }

    fn generate_fallback_tile(&self) -> Vec<f32> {
        let mut data = vec![0.0f32; (self.tile_size * self.tile_size) as usize];
        for y in 0..self.tile_size {
            for x in 0..self.tile_size {
                data[(y * self.tile_size + x) as usize] = if (x + y) % 2 == 0 { 255.0 } else { 0.0 };
            }
        }
        data
    }
}

impl Custom3d {
    fn new(wgpu_render_state: &egui_wgpu::RenderState, meta: &MetaResponse) -> Self {
        let device = &wgpu_render_state.device;
        let tile_manager = GpuTileManager::new(meta.tile_size);

        let texture_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                    count: None,
                },
            ],
            label: Some("texture_bind_group_layout"),
        });

        let uniform_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<Uniforms>() as u64),
                    },
                    count: None,
                },
            ],
            label: Some("uniform_bind_group_layout"),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Render Pipeline Layout"),
            bind_group_layouts: &[&texture_bind_group_layout, &uniform_bind_group_layout],
            push_constant_ranges: &[],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[Vertex::desc()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu_render_state.target_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Vertex Buffer"),
            contents: bytemuck::cast_slice(VERTICES),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Index Buffer"),
            contents: bytemuck::cast_slice(INDICES),
            usage: wgpu::BufferUsages::INDEX,
        });
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Uniform Buffer"),
            size: (256 * 64), // 64 tiles with 256 byte alignment
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &uniform_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &uniform_buffer,
                        offset: 0,
                        size: wgpu::BufferSize::new(std::mem::size_of::<Uniforms>() as u64),
                    }),
                },
            ],
            label: Some("uniform_bind_group"),
        });

        wgpu_render_state.renderer.write().paint_callback_resources.insert(Custom3dResources {
            pipeline,
            vertex_buffer,
            index_buffer,
            num_indices: INDICES.len() as u32,
            uniform_bind_group,
            uniform_buffer,
            tile_manager,
            texture_bind_group_layout,
        });

        Self
    }
}

struct FitsViewApp {
    meta: Option<MetaResponse>,
    _renderer: Option<Custom3d>,
    backend_url: String,
    viewport: SimpleViewport,
}

impl Default for FitsViewApp {
    fn default() -> Self {
        Self { meta: None, _renderer: None, backend_url: "http://127.0.0.1:8001".to_string(), viewport: SimpleViewport::default() }
    }
}

impl FitsViewApp {
    fn new(cc: &eframe::CreationContext<'_>, backend_url: String, initial_pan: egui::Vec2, initial_zoom: f32) -> Self {
        let meta = fetch_meta(&backend_url);
        let _renderer = meta.as_ref().map(|m| Custom3d::new(cc.wgpu_render_state.as_ref().expect("wgpu backend"), m));

        let mut viewport = SimpleViewport::default();
        if let Some(meta) = &meta {
            let image_size = egui::Vec2::new(meta.shape[1] as f32, meta.shape[0] as f32);
            viewport.fit_to_window(cc.egui_ctx.screen_rect().size(), image_size);
            viewport.zoom = initial_zoom;
            viewport.center_on_image = image_size * 0.5 - initial_pan;
        }

        Self { meta, _renderer, backend_url, viewport }
    }

    fn handle_input(&mut self, response: &egui::Response, ctx: &egui::Context) {
        if response.dragged() {
            let delta = response.drag_delta();
            let pan_in_image = delta / self.viewport.zoom;
            self.viewport.center_on_image.x -= pan_in_image.x;
            self.viewport.center_on_image.y += pan_in_image.y;
        }

        if response.hovered() {
            ctx.input(|i| {
                let scroll_delta = i.scroll_delta.y;
                if scroll_delta != 0.0 {
                    let zoom_factor = 1.0 + scroll_delta * 0.001;
                    let new_zoom = (self.viewport.zoom * zoom_factor).clamp(0.1, 10.0);

                    if let Some(hover_pos) = i.pointer.hover_pos() {
                        let image_pos_before_zoom = screen_to_image_local(hover_pos, &response.rect, &self.viewport);
                        self.viewport.zoom = new_zoom;
                        let image_pos_after_zoom = screen_to_image_local(hover_pos, &response.rect, &self.viewport);
                        self.viewport.center_on_image += image_pos_before_zoom - image_pos_after_zoom;
                    }
                }
            });
        }

        ctx.input(|i| {
            if i.key_pressed(egui::Key::R) {
                if let Some(meta) = &self.meta {
                    let image_size = egui::Vec2::new(meta.shape[1] as f32, meta.shape[0] as f32);
                    self.viewport.reset(image_size);
                }
            }
        });
    }
}

fn screen_to_image_local(screen_pos: egui::Pos2, rect: &egui::Rect, viewport: &SimpleViewport) -> egui::Vec2 {
    let ndc_x = (screen_pos.x - rect.min.x) / rect.width() * 2.0 - 1.0;
    let ndc_y = 1.0 - (screen_pos.y - rect.min.y) / rect.height() * 2.0;
    let scale_x = (2.0 * viewport.zoom) / rect.width();
    let scale_y = (2.0 * viewport.zoom) / rect.height();
    egui::Vec2::new(ndc_x / scale_x + viewport.center_on_image.x, ndc_y / scale_y + viewport.center_on_image.y)
}

impl eframe::App for FitsViewApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("FITS View");
            if let Some(meta) = &self.meta {
                ui.label(format!("Shape: {}x{}", meta.shape[1], meta.shape[0]));
                ui.label(format!("Zoom: {:.2}x, Center: ({:.1}, {:.1}) px", self.viewport.zoom, self.viewport.center_on_image.x, self.viewport.center_on_image.y));
            } else {
                ui.label("Failed to fetch metadata.");
            }

            let (rect, response) = ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
            self.handle_input(&response, ctx);

            if let Some(meta) = &self.meta {
                let image_size = ImageSize { width: meta.shape[1] as f32, height: meta.shape[0] as f32 };
                let render_size = RenderSize { width: rect.width(), height: rect.height() };

                let core_viewport = Viewport {
                    zoom: self.viewport.zoom,
                    center_on_image: [self.viewport.center_on_image.x, self.viewport.center_on_image.y],
                    rotation_angle: self.viewport.rotation_angle,
                };

                let visible_tiles = calculate_visible_tiles(&core_viewport, render_size, image_size, meta.tile_size, calculate_max_lod(image_size, meta.tile_size));
                let transform_matrix = create_image_to_ndc_matrix(&core_viewport, render_size);

                let backend_url_clone = self.backend_url.clone();
                let meta_clone = meta.clone();
                let visible_tiles_clone = visible_tiles.clone();

                let callback = egui::PaintCallback {
                    rect,
                    callback: Arc::new(egui_wgpu::CallbackFn::new()
                        .prepare(move |device, queue, _encoder, resources| {
                            let resources: &mut Custom3dResources = resources.get_mut().unwrap();
                            for tile_coord in &visible_tiles {
                                resources.tile_manager.get_or_create_tile(tile_coord, device, queue, &backend_url_clone, &resources.texture_bind_group_layout);
                            }

                            let mut tile_index = 0;
                            for tile_coord in &visible_tiles {
                                if resources.tile_manager.tiles.contains_key(tile_coord) {
                                    let lod_scale = 2_u32.pow(tile_coord.lod) as f32;
                                    let effective_tile_size = resources.tile_manager.tile_size as f32 * lod_scale;
                                    let tile_offset_x = tile_coord.x as f32 * effective_tile_size;
                                    let tile_offset_y = tile_coord.y as f32 * effective_tile_size;

                                    let uniforms = Uniforms {
                                        transform: transform_matrix,
                                        tile_offset: [tile_offset_x, tile_offset_y],
                                        vmin: meta_clone.min_val,
                                        vmax: meta_clone.max_val,
                                        tile_size: effective_tile_size,
                                        _padding1: 0.0,
                                        _padding2: [0.0, 0.0],
                                        _padding3: [0.0, 0.0, 0.0, 0.0],
                                    };
                                    queue.write_buffer(&resources.uniform_buffer, (tile_index * 256) as u64, bytemuck::cast_slice(&[uniforms]));
                                    tile_index += 1;
                                }
                            }
                            Vec::new()
                        })
                        .paint(move |_info, rpass, resources| {
                            let resources: &Custom3dResources = resources.get().unwrap();
                            rpass.set_pipeline(&resources.pipeline);
                            rpass.set_vertex_buffer(0, resources.vertex_buffer.slice(..));
                            rpass.set_index_buffer(resources.index_buffer.slice(..), wgpu::IndexFormat::Uint16);

                            let mut tile_index = 0;
                            for tile_coord in &visible_tiles_clone {
                                if let Some(tile_data) = resources.tile_manager.tiles.get(tile_coord) {
                                    rpass.set_bind_group(0, &tile_data.bind_group, &[]);
                                    rpass.set_bind_group(1, &resources.uniform_bind_group, &[(tile_index * 256) as u32]);
                                    rpass.draw_indexed(0..resources.num_indices, 0, 0..1);
                                    tile_index += 1;
                                }
                            }
                        })
                    ),
                };
                ui.painter().add(callback);
            }
        });
    }
}

fn main() -> eframe::Result<()> {
    let args = Args::parse();
    let native_options = eframe::NativeOptions { renderer: eframe::Renderer::Wgpu, ..Default::default() };
    eframe::run_native("FITS View", native_options, Box::new(move |cc| Box::new(FitsViewApp::new(cc, args.backend_url, egui::Vec2::new(args.initial_pan_x, args.initial_pan_y), args.initial_zoom))))
}

fn fetch_meta(backend_url: &str) -> Option<MetaResponse> {
    let url = format!("{}/meta", backend_url);
    match reqwest::blocking::get(url) {
        Ok(response) => response.json::<MetaResponse>().ok(),
        Err(_) => None,
    }
}

fn fetch_tile_data(backend_url: &str, z: u32, x: u32, y: u32) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    let url = format!("{}/tile/{}/{}/{}?format=raw", backend_url, z, x, y);
    let response = reqwest::blocking::get(url)?;
    let bytes = response.bytes()?;
    let (_header, payload) = raw_header::RawTileHeader::from_prefix(&bytes)?;
    let f32_data: Vec<f32> = payload.chunks_exact(4).map(|c| f32::from_le_bytes([c[0],c[1],c[2],c[3]])).collect();
    Ok(f32_data)
}
