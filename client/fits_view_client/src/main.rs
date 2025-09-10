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
use tokio::runtime::Runtime;
use tokio::sync::mpsc::{self, UnboundedSender, UnboundedReceiver};

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
            attributes: &
                [
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

struct GpuTileData {
    _texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
}

enum TileState {
    Loading,
    Loaded(GpuTileData),
    Fallback(GpuTileData),
}

struct GpuTileManager {
    tiles: std::collections::HashMap<TileCoord, TileState>,
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

    fn get_tile<'a>(&'a mut self, coord: &TileCoord, runtime: &Runtime, backend_url: &str, tile_sender: &UnboundedSender<(TileCoord, Vec<f32>)>) -> &'a TileState {
        if !self.tiles.contains_key(coord) {
            self.tiles.insert(coord.clone(), TileState::Loading);
            let backend_url = backend_url.to_string();
            let coord = coord.clone();
            let tile_sender = tile_sender.clone();
            runtime.spawn(async move {
                match fetch_tile_data(&backend_url, coord.lod, coord.x, coord.y).await {
                    Ok(data) => {
                        let _ = tile_sender.send((coord, data));
                    }
                    Err(e) => {
                        println!("Failed to fetch tile ({}, {}, {}): {}", coord.lod, coord.x, coord.y, e);
                    }
                }
            });
        }
        self.tiles.get(coord).unwrap()
    }

    fn upload_tile_data(&self, f32_data: &[f32], device: &wgpu::Device, queue: &wgpu::Queue, texture_bind_group_layout: &wgpu::BindGroupLayout) -> GpuTileData {
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
            bytemuck::cast_slice(f32_data),
            wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(4 * self.tile_size), rows_per_image: Some(self.tile_size) },
            texture_size,
        );

        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor { mag_filter: wgpu::FilterMode::Nearest, min_filter: wgpu::FilterMode::Nearest, ..Default::default() });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: texture_bind_group_layout,
            entries: &
                [
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&texture_view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
                ],
            label: Some("tile_bind_group"),
        });

        GpuTileData { _texture: texture, bind_group }
    }
}

impl Custom3d {
    fn new(wgpu_render_state: &egui_wgpu::RenderState, meta: &MetaResponse) -> Self {
        let device = &wgpu_render_state.device;
        let tile_manager = GpuTileManager::new(meta.tile_size);

        let texture_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &
                [
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
            entries: &
                [
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
                targets: &
                    [
                        Some(wgpu::ColorTargetState {
                            format: wgpu_render_state.target_format,
                            blend: Some(wgpu::BlendState::REPLACE),
                            write_mask: wgpu::ColorWrites::ALL,
                        }),
                    ],
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
            entries: &
                [
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
    show_debug_overlay: bool,
    runtime: Arc<Runtime>,
    tile_sender: UnboundedSender<(TileCoord, Vec<f32>)>, 
    tile_receiver: UnboundedReceiver<(TileCoord, Vec<f32>)>, 
}

impl FitsViewApp {
    fn new(cc: &eframe::CreationContext<'_>, backend_url: String, initial_pan: egui::Vec2, initial_zoom: f32) -> Self {
        let meta = fetch_meta_blocking(&backend_url);
        let _renderer = meta.as_ref().map(|m| Custom3d::new(cc.wgpu_render_state.as_ref().expect("wgpu backend"), m));

        let mut viewport = SimpleViewport::default();
        if let Some(meta) = &meta {
            let image_size = egui::Vec2::new(meta.shape[1] as f32, meta.shape[0] as f32);
            viewport.fit_to_window(cc.egui_ctx.screen_rect().size(), image_size);
            viewport.zoom = initial_zoom;
            viewport.center_on_image = image_size * 0.5 - initial_pan;
        }

        let runtime = Arc::new(tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap());
        let (tile_sender, tile_receiver) = mpsc::unbounded_channel();

        Self { meta, _renderer, backend_url, viewport, show_debug_overlay: false, runtime, tile_sender, tile_receiver }
    }

    fn handle_input(&mut self, response: &egui::Response, ctx: &egui::Context, frame: &mut eframe::Frame) {
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
            if i.key_pressed(egui::Key::Q) {
                frame.close();
            }
            if i.key_pressed(egui::Key::D) {
                self.show_debug_overlay = !self.show_debug_overlay;
            }
        });
    }

    fn draw_debug_overlay(&self, ui: &mut egui::Ui, rect: egui::Rect, visible_tiles: &[TileCoord], meta: &MetaResponse) {
        let painter = ui.painter();
        
        // Draw render screen boundary (black rectangle)
        painter.rect_stroke(rect, 0.0, egui::Stroke::new(2.0, egui::Color32::BLACK));
        
        // Calculate image screen extent
        let image_size = ImageSize { width: meta.shape[1] as f32, height: meta.shape[0] as f32 };
        let render_size = RenderSize { width: rect.width(), height: rect.height() };
        let core_viewport = Viewport {
            zoom: self.viewport.zoom,
            center_on_image: [self.viewport.center_on_image.x, self.viewport.center_on_image.y],
            rotation_angle: self.viewport.rotation_angle,
        };
        
        // Calculate image extent in screen coordinates
        let image_extent = fits_view_client::calculate_image_screen_extent(&core_viewport, render_size, image_size);
        
        // Convert from Y-up coordinate system to egui's Y-down coordinate system
        let extent_min_x = rect.min.x + image_extent.min[0] as f32;
        let extent_min_y = rect.min.y + (rect.height() - image_extent.max[1] as f32);
        let extent_width = (image_extent.max[0] - image_extent.min[0]) as f32;
        let extent_height = (image_extent.max[1] - image_extent.min[1]) as f32;
        
        let extent_rect = egui::Rect::from_min_size(
            egui::Pos2::new(extent_min_x, extent_min_y),
            egui::Vec2::new(extent_width, extent_height)
        );
        
        // Draw image screen extent (red dashed rectangle)
        painter.rect_stroke(extent_rect, 0.0, egui::Stroke::new(2.0, egui::Color32::RED));
        
        // Draw visible tiles
        for tile_coord in visible_tiles {
            let lod_scale = 2_u32.pow(tile_coord.lod) as f32;
            let effective_tile_size = meta.tile_size as f32 * lod_scale;
            
            // Calculate tile position in image coordinates
            let tile_image_x = tile_coord.x as f32 * effective_tile_size;
            let tile_image_y = tile_coord.y as f32 * effective_tile_size;
            
            // Transform to screen coordinates using the same logic as the shader
            let scale_x = (2.0 * self.viewport.zoom) / rect.width();
            let scale_y = (2.0 * self.viewport.zoom) / rect.height();
            
            // Convert image coordinates to NDC, then to screen coordinates
            let ndc_x = (tile_image_x - self.viewport.center_on_image.x) * scale_x;
            let ndc_y = (tile_image_y - self.viewport.center_on_image.y) * scale_y;
            
            let screen_x = rect.min.x + (ndc_x + 1.0) * rect.width() / 2.0;
            let screen_y = rect.min.y + (1.0 - ndc_y) * rect.height() / 2.0; // Flip Y for screen coordinates
            
            let tile_screen_size = effective_tile_size * self.viewport.zoom;
            
            let tile_rect = egui::Rect::from_min_size(
                egui::Pos2::new(screen_x, screen_y - tile_screen_size), // Adjust for Y-up to Y-down conversion
                egui::Vec2::new(tile_screen_size, tile_screen_size)
            );
            
            // Draw tile boundary (green for visible tiles)
            painter.rect_stroke(tile_rect, 0.0, egui::Stroke::new(1.0, egui::Color32::GREEN));
            
            // Draw tile coordinates
            let text = format!("({},{}, M{})", tile_coord.x, tile_coord.y, tile_coord.lod);
            painter.text(
                tile_rect.center(),
                egui::Align2::CENTER_CENTER,
                text,
                egui::FontId::monospace(10.0),
                egui::Color32::WHITE
            );
        }
        
        // Draw debug info text
        let debug_text = format!(
            "Debug Overlay (D to toggle)\nZoom: {:.2}x\nCenter: ({:.1}, {:.1}) px\nVisible Tiles: {}",
            self.viewport.zoom,
            self.viewport.center_on_image.x,
            self.viewport.center_on_image.y,
            visible_tiles.len()
        );
        
        painter.text(
            rect.min + egui::Vec2::new(10.0, 10.0),
            egui::Align2::LEFT_TOP,
            debug_text,
            egui::FontId::monospace(12.0),
            egui::Color32::YELLOW
        );
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
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let mut received_tiles = vec![];
        while let Ok(tile) = self.tile_receiver.try_recv() {
            received_tiles.push(tile);
        }

        if !received_tiles.is_empty() {
            if let Some(wgpu_render_state) = frame.wgpu_render_state() {
                let mut renderer = wgpu_render_state.renderer.write();
                let resources: &mut Custom3dResources = renderer.paint_callback_resources.get_mut().unwrap();
                for (coord, data) in received_tiles {
                    let device = &wgpu_render_state.device;
                    let queue = &wgpu_render_state.queue;
                    let gpu_tile_data = resources.tile_manager.upload_tile_data(&data, device, queue, &resources.texture_bind_group_layout);
                    resources.tile_manager.tiles.insert(coord, TileState::Loaded(gpu_tile_data));
                }
            }
            ctx.request_repaint();
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("FITS View");
            if let Some(meta) = &self.meta {
                ui.label(format!("Shape: {}x{}", meta.shape[1], meta.shape[0]));
                ui.label(format!("Zoom: {:.2}x, Center: ({:.1}, {:.1}) px", self.viewport.zoom, self.viewport.center_on_image.x, self.viewport.center_on_image.y));
            } else {
                ui.label("Failed to fetch metadata.");
            }

            let (rect, response) = ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
            self.handle_input(&response, ctx, frame);

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
                let visible_tiles_debug = visible_tiles.clone();
                let tile_sender_clone = self.tile_sender.clone();
                let runtime_clone = self.runtime.clone();

                let callback = egui::PaintCallback {
                    rect,
                    callback: Arc::new(egui_wgpu::CallbackFn::new()
                        .prepare(move |_device, queue, _encoder, resources| {
                            let resources: &mut Custom3dResources = resources.get_mut().unwrap();
                            for tile_coord in &visible_tiles {
                                resources.tile_manager.get_tile(tile_coord, &runtime_clone, &backend_url_clone, &tile_sender_clone);
                            }

                            let mut tile_index = 0;
                            for tile_coord in &visible_tiles {
                                if let Some(tile_state) = resources.tile_manager.tiles.get(tile_coord) {
                                    if let TileState::Loaded(_) | TileState::Fallback(_) = tile_state {
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
                                if let Some(tile_state) = resources.tile_manager.tiles.get(tile_coord) {
                                    let bind_group = match tile_state {
                                        TileState::Loaded(gpu_tile_data) => Some(&gpu_tile_data.bind_group),
                                        TileState::Fallback(gpu_tile_data) => Some(&gpu_tile_data.bind_group),
                                        TileState::Loading => None,
                                    };
                                    if let Some(bind_group) = bind_group {
                                        rpass.set_bind_group(0, bind_group, &[]);
                                        rpass.set_bind_group(1, &resources.uniform_bind_group, &[(tile_index * 256) as u32]);
                                        rpass.draw_indexed(0..resources.num_indices, 0, 0..1);
                                        tile_index += 1;
                                    }
                                }
                            }
                        })
                    ),
                };
                ui.painter().add(callback);
                
                if self.show_debug_overlay {
                    self.draw_debug_overlay(ui, rect, &visible_tiles_debug, meta);
                }
            }
        });
    }
}

fn main() -> eframe::Result<()> {
    let args = Args::parse();
    let native_options = eframe::NativeOptions { renderer: eframe::Renderer::Wgpu, ..Default::default() };
    eframe::run_native("FITS View", native_options, Box::new(move |cc| Box::new(FitsViewApp::new(cc, args.backend_url, egui::Vec2::new(args.initial_pan_x, args.initial_pan_y), args.initial_zoom))))
}

fn fetch_meta_blocking(backend_url: &str) -> Option<MetaResponse> {
    let url = format!("{}/meta", backend_url);
    // Using a blocking call here for simplicity during initialization.
    // In a real-world app, you might want to make this async as well and show a loading screen.
    match reqwest::blocking::get(url) {
        Ok(response) => response.json::<MetaResponse>().ok(),
        Err(_) => None,
    }
}

async fn fetch_tile_data(backend_url: &str, z: u32, x: u32, y: u32) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    let url = format!("{}/tile/{}/{}/{}?format=raw", backend_url, z, x, y);
    let response = reqwest::get(&url).await?;
    let bytes = response.bytes().await?;
    let (_header, payload) = raw_header::RawTileHeader::from_prefix(&bytes)?;
    let f32_data: Vec<f32> = payload.chunks_exact(4).map(|c| f32::from_le_bytes([c[0],c[1],c[2],c[3]])).collect();
    Ok(f32_data)
}
