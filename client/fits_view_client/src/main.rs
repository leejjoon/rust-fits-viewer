mod raw_header;
mod parse;
mod upload;

use clap::Parser;
use eframe::egui;
use serde::Deserialize;
use std::sync::Arc;
use wgpu::util::DeviceExt;
use crate::parse::parse_raw_tile;
use crate::upload::upload_r32float_texture;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Backend server URL
    #[arg(long, default_value = "http://127.0.0.1:8001")]
    backend_url: String,
}

#[derive(Deserialize, Debug, Clone)]
struct MetaResponse {
    shape: [u32; 2],
    dtype: String,
    tile_size: u32,
    zoom_levels: u32,
    min_val: f32,
    max_val: f32,
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
    transform: [[f32; 4]; 4], // 4x4 transformation matrix
    tile_offset: [f32; 2], // Tile position offset in image coordinates
    vmin: f32,
    vmax: f32,
    _padding: [f32; 2], // Align to 16 bytes
}

#[derive(Clone, Debug)]
struct Viewport {
    zoom: f32,
    pan_offset: egui::Vec2,
    rotation_angle: f32,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct TileCoord {
    z: u32,
    x: u32,
    y: u32,
}

struct TileData {
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    coord: TileCoord,
}

struct TileManager {
    tiles: std::collections::HashMap<TileCoord, TileData>,
    tile_size: u32,
    image_size: [u32; 2],
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            pan_offset: egui::Vec2::ZERO,
            rotation_angle: 0.0,
        }
    }
}

impl Viewport {
    fn reset(&mut self) {
        *self = Self::default();
    }
    
    fn fit_to_window(&mut self, window_size: egui::Vec2, image_size: egui::Vec2) {
        let scale_x = window_size.x / image_size.x;
        let scale_y = window_size.y / image_size.y;
        self.zoom = scale_x.min(scale_y);
        self.pan_offset = egui::Vec2::ZERO;
        self.rotation_angle = 0.0;
    }
    
    fn get_visible_tiles(&self, render_size: egui::Vec2, image_size: egui::Vec2, tile_size: u32) -> Vec<TileCoord> {
        let mut visible_tiles = Vec::new();
        
        // Calculate the bounds of the visible area in image coordinates
        let half_render = render_size * 0.5;
        let image_center = image_size * 0.5;
        
        // Transform viewport bounds to image coordinates
        let zoom_inv = 1.0 / self.zoom;
        let visible_half_width = half_render.x * zoom_inv;
        let visible_half_height = half_render.y * zoom_inv;
        
        // For tile visibility, we need to be more generous and show a larger area
        // to account for the fact that tiles might be partially visible
        // Use a larger viewport that encompasses the full render area plus some padding
        let padding_factor = 1.5; // Show 50% more area to catch edge tiles
        let expanded_half_width = visible_half_width * padding_factor;
        let expanded_half_height = visible_half_height * padding_factor;
        
        // Apply pan offset to determine which part of the image is visible
        let pan_in_image_x = -self.pan_offset.x * render_size.x * 0.5 * zoom_inv;
        let pan_in_image_y = self.pan_offset.y * render_size.y * 0.5 * zoom_inv;
        
        let center_x = image_center.x + pan_in_image_x;
        let center_y = image_center.y + pan_in_image_y;
        
        let min_x = (center_x - expanded_half_width).max(0.0);
        let max_x = (center_x + expanded_half_width).min(image_size.x);
        let min_y = (center_y - expanded_half_height).max(0.0);
        let max_y = (center_y + expanded_half_height).min(image_size.y);
        
        // Convert to tile coordinates
        let tile_size_f = tile_size as f32;
        let min_tile_x = (min_x / tile_size_f).floor() as u32;
        let max_tile_x = (max_x / tile_size_f).ceil() as u32;
        let min_tile_y = (min_y / tile_size_f).floor() as u32;
        let max_tile_y = (max_y / tile_size_f).ceil() as u32;
        
        
        // Generate tile coordinates (zoom level 0 for now)
        let max_tiles_x = (image_size.x as u32 + tile_size - 1) / tile_size;
        let max_tiles_y = (image_size.y as u32 + tile_size - 1) / tile_size;
        
        let mut all_requested = Vec::new();
        for y in min_tile_y..max_tile_y {
            for x in min_tile_x..max_tile_x {
                all_requested.push((x, y));
                if x < max_tiles_x && y < max_tiles_y {
                    visible_tiles.push(TileCoord { z: 0, x, y });
                }
            }
        }
        
        
        
        visible_tiles
    }
    
    fn to_transform_matrix(&self, render_size: egui::Vec2, image_size: egui::Vec2) -> [[f32; 4]; 4] {
        // Column-major 2D transformation matrix for WGSL
        let cos_r = self.rotation_angle.cos();
        let sin_r = self.rotation_angle.sin();
        
        // Convert from image pixels to normalized device coordinates [-1,1]
        let pixels_to_ndc_x = 2.0 / render_size.x;
        let pixels_to_ndc_y = 2.0 / render_size.y;
        
        // Scale by zoom factor only (tile size scaling happens in shader)
        let scale_x = self.zoom * pixels_to_ndc_x;
        let scale_y = self.zoom * pixels_to_ndc_y;
        
        // No automatic centering - let the image render from origin (0,0)
        // The user can pan to center the image as needed
        let total_offset_x = self.pan_offset.x;
        let total_offset_y = self.pan_offset.y;
        
        // Column-major matrix
        [
            [scale_x * cos_r, scale_y * sin_r, 0.0, 0.0],      // Column 0
            [-scale_x * sin_r, scale_y * cos_r, 0.0, 0.0],     // Column 1
            [0.0, 0.0, 1.0, 0.0],                               // Column 2
            [total_offset_x, total_offset_y, 0.0, 1.0],        // Column 3 (translation)
        ]
    }
}


struct Custom3dResources {
    pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    num_indices: u32,
    uniform_bind_group: wgpu::BindGroup,
    uniform_buffer: wgpu::Buffer,
    tile_manager: TileManager,
    texture_bind_group_layout: wgpu::BindGroupLayout,
}

struct Custom3d {
    backend_url: String,
}

impl TileManager {
    fn new(tile_size: u32, image_size: [u32; 2]) -> Self {
        Self {
            tiles: std::collections::HashMap::new(),
            tile_size,
            image_size,
        }
    }
    
    fn get_or_create_tile(
        &mut self,
        coord: TileCoord,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        backend_url: &str,
        texture_bind_group_layout: &wgpu::BindGroupLayout,
    ) -> Option<&TileData> {
        if !self.tiles.contains_key(&coord) {
            // Fetch tile data
            if let Ok(tile_bytes) = fetch_tile_data(backend_url, coord.z, coord.x, coord.y) {
                if let Ok(tile) = parse_raw_tile(&tile_bytes) {
                    let texture = upload_r32float_texture(device, queue, &tile);
                    let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                    
                    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
                        address_mode_u: wgpu::AddressMode::ClampToEdge,
                        address_mode_v: wgpu::AddressMode::ClampToEdge,
                        address_mode_w: wgpu::AddressMode::ClampToEdge,
                        mag_filter: wgpu::FilterMode::Nearest,
                        min_filter: wgpu::FilterMode::Nearest,
                        mipmap_filter: wgpu::FilterMode::Nearest,
                        ..Default::default()
                    });
                    
                    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                        layout: texture_bind_group_layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(&texture_view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: wgpu::BindingResource::Sampler(&sampler),
                            },
                        ],
                        label: Some(&format!("tile_bind_group_{}_{}", coord.x, coord.y)),
                    });
                    
                    let tile_data = TileData {
                        texture,
                        bind_group,
                        coord: coord.clone(),
                    };
                    
                    self.tiles.insert(coord.clone(), tile_data);
                } else {
                    eprintln!("Failed to parse tile ({}, {})", coord.x, coord.y);
                    return None;
                }
            } else {
                eprintln!("Failed to fetch tile ({}, {})", coord.x, coord.y);
                return None;
            }
        }
        
        self.tiles.get(&coord)
    }
    
    fn get_tile_offset(&self, coord: &TileCoord) -> [f32; 2] {
        let tile_size_f = self.tile_size as f32;
        let tile_x = coord.x as f32 * tile_size_f;
        let tile_y = coord.y as f32 * tile_size_f;
        [tile_x, tile_y]
    }
}

impl Custom3d {
    fn new(wgpu_render_state: &egui_wgpu::RenderState, backend_url: String, meta: Option<&MetaResponse>) -> Self {
        let device = &wgpu_render_state.device;
        let _queue = &wgpu_render_state.queue;

        // Get normalization values from metadata
        let (vmin, vmax) = if let Some(meta) = meta {
            (meta.min_val, meta.max_val)
        } else {
            (0.0, 255.0) // Default fallback values
        };


        // Initialize tile manager
        let (tile_size, image_size) = if let Some(meta) = meta {
            (meta.tile_size, meta.shape)
        } else {
            (256, [1024, 1024]) // Default values
        };
        
        let tile_manager = TileManager::new(tile_size, image_size);

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

        // Note: Individual tile bind groups will be created by TileManager

        let identity_matrix = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        
        // Create a larger uniform buffer to hold multiple tile uniforms
        // Allocate space for up to 64 tiles with 256-byte alignment
        let max_tiles = 64;
        let uniform_buffer_size = max_tiles * 256;
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Multi-Tile Uniform Buffer"),
            size: uniform_buffer_size as u64,
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

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        let render_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Render Pipeline Layout"),
            bind_group_layouts: &[&texture_bind_group_layout, &uniform_bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Render Pipeline"),
            layout: Some(&render_pipeline_layout),
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
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
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
        let num_indices = INDICES.len() as u32;

        // Store resources in the renderer's callback resources
        wgpu_render_state
            .renderer
            .write()
            .paint_callback_resources
            .insert(Custom3dResources {
                pipeline,
                vertex_buffer,
                index_buffer,
                num_indices,
                uniform_bind_group,
                uniform_buffer,
                tile_manager,
                texture_bind_group_layout,
            });

        Self { backend_url }
    }
}


struct FitsViewApp {
    meta: Option<MetaResponse>,
    renderer: Option<Custom3d>,
    backend_url: String,
    viewport: Viewport,
}

impl Default for FitsViewApp {
    fn default() -> Self {
        Self {
            meta: None,
            renderer: None,
            backend_url: "http://127.0.0.1:8001".to_string(),
            viewport: Viewport::default(),
        }
    }
}

impl FitsViewApp {
    fn new(cc: &eframe::CreationContext<'_>, backend_url: String) -> Self {
        let meta = fetch_meta(&backend_url);
        let renderer = {
            let wgpu_render_state = cc.wgpu_render_state.as_ref().expect("wgpu backend");
            Some(Custom3d::new(wgpu_render_state, backend_url.clone(), meta.as_ref()))
        };
        
        // Initialize viewport with proper centering
        let mut viewport = Viewport::default();
        if let Some(meta) = &meta {
            // Use a reasonable default window size for initial centering
            let window_size = egui::Vec2::new(800.0, 600.0);
            let image_size = egui::Vec2::new(meta.shape[0] as f32, meta.shape[1] as f32);
            viewport.fit_to_window(window_size, image_size);
        }
        
        Self { meta, renderer, backend_url, viewport }
    }
    
    fn handle_input(&mut self, response: &egui::Response, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // Mouse drag for panning
        if response.dragged() {
            let mut delta = response.drag_delta();
            // Fix inverted Y-axis: flip Y component so dragging up moves image up
            delta.y = -delta.y;
            
            // Use the response rect (actual rendering area) instead of full screen
            let render_rect = response.rect;
            // Scale to normalized device coordinates [-1,1] based on actual render area
            // This ensures 1:1 pixel-to-coordinate mapping within the render area
            let sensitivity = egui::Vec2::new(
                2.0 / render_rect.width(), 
                2.0 / render_rect.height()
            );
            self.viewport.pan_offset += delta * sensitivity;
        }
        
        // Handle scroll events for zoom and rotation
        ctx.input(|i| {
            let scroll_delta = i.scroll_delta.y;
            
            if scroll_delta != 0.0 {
                // Check if we're hovering over the response area
                if let Some(hover_pos) = response.hover_pos() {
                    if i.modifiers.alt {
                        // Alt + scroll for rotation (clockwise/counter-clockwise)
                        // Positive scroll_delta = scroll up = counter-clockwise rotation
                        // Negative scroll_delta = scroll down = clockwise rotation
                        self.viewport.rotation_angle += scroll_delta * 0.005;
                    } else {
                        // Regular scroll for zoom - zoom around mouse position
                        let zoom_factor = 1.0 + scroll_delta * 0.001;
                        let new_zoom = (self.viewport.zoom * zoom_factor).clamp(0.1, 10.0);
                        
                        if new_zoom != self.viewport.zoom {
                            // Get mouse position in screen coordinates relative to render area
                            let render_rect = response.rect;
                            let mouse_screen_x = hover_pos.x - render_rect.min.x;
                            let mouse_screen_y = hover_pos.y - render_rect.min.y;
                            
                            // Convert to image coordinates by reversing the current transformation
                            let render_size = egui::Vec2::new(render_rect.width(), render_rect.height());
                            
                            // Convert screen position to NDC [-1, 1]
                            let mouse_ndc_x = (mouse_screen_x / render_rect.width()) * 2.0 - 1.0;
                            let mouse_ndc_y = -((mouse_screen_y / render_rect.height()) * 2.0 - 1.0);
                            
                            // Convert NDC to image pixels using current transform
                            let pixels_to_ndc_x = 2.0 / render_size.x;
                            let pixels_to_ndc_y = 2.0 / render_size.y;
                            
                            // Reverse the transformation to get image coordinates
                            let image_x = (mouse_ndc_x - self.viewport.pan_offset.x) / (self.viewport.zoom * pixels_to_ndc_x);
                            let image_y = (mouse_ndc_y - self.viewport.pan_offset.y) / (self.viewport.zoom * pixels_to_ndc_y);
                            
                            // Apply new zoom
                            let old_zoom = self.viewport.zoom;
                            self.viewport.zoom = new_zoom;
                            
                            // Calculate new pan offset to keep the same image point under the mouse
                            let new_ndc_x = image_x * (self.viewport.zoom * pixels_to_ndc_x);
                            let new_ndc_y = image_y * (self.viewport.zoom * pixels_to_ndc_y);
                            
                            self.viewport.pan_offset.x = mouse_ndc_x - new_ndc_x;
                            self.viewport.pan_offset.y = mouse_ndc_y - new_ndc_y;
                        }
                    }
                }
            }
        });
        
        // Keyboard shortcuts
        ctx.input(|i| {
            if i.key_pressed(egui::Key::R) {
                self.viewport.reset();
            }
            if i.key_pressed(egui::Key::F) {
                if let Some(meta) = &self.meta {
                    let window_size = egui::Vec2::new(800.0, 600.0); // Default size
                    let image_size = egui::Vec2::new(meta.shape[0] as f32, meta.shape[1] as f32);
                    self.viewport.fit_to_window(window_size, image_size);
                }
            }
            if i.key_pressed(egui::Key::Q) {
                frame.close();
            }
        });
    }
}

fn fetch_meta(backend_url: &str) -> Option<MetaResponse> {
    let url = format!("{}/meta", backend_url);
    match reqwest::blocking::get(url) {
        Ok(response) => {
            if response.status().is_success() {
                match response.json::<MetaResponse>() {
                    Ok(meta) => Some(meta),
                    Err(_) => None,
                }
            } else {
                eprintln!("Meta request failed with status: {}", response.status());
                None
            }
        }
        Err(_) => {
            eprintln!("Failed to fetch meta");
            None
        }
    }
}

fn fetch_tile_data(backend_url: &str, z: u32, x: u32, y: u32) -> Result<Vec<u8>, String> {
    let url = format!("{}/tile/{}/{}/{}?format=raw&dtype=float32", backend_url, z, x, y);
    match reqwest::blocking::get(&url) {
        Ok(response) => {
            if response.status().is_success() {
                match response.bytes() {
                    Ok(bytes) => Ok(bytes.to_vec()),
                    Err(_) => Err("Failed to read tile response".to_string()),
                }
            } else {
                Err(format!("Tile request failed with status: {}", response.status()))
            }
        }
        Err(_) => {
            Err("Failed to fetch tile".to_string())
        }
    }
}

fn create_test_texture(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::Texture {
    let texture_size = wgpu::Extent3d { width: 8, height: 8, depth_or_array_layers: 1 };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        size: texture_size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        label: Some("test_texture"),
        view_formats: &[],
    });

    let checkerboard = {
        let mut data = Vec::with_capacity((texture_size.width * texture_size.height) as usize * 4);
        for i in 0..texture_size.width {
            for j in 0..texture_size.height {
                let is_white = (i + j) % 2 == 0;
                data.extend_from_slice(if is_white { &[255, 255, 255, 255] } else { &[0, 0, 0, 255] });
            }
        }
        data
    };

    queue.write_texture(
        wgpu::ImageCopyTexture {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &checkerboard,
        wgpu::ImageDataLayout {
            offset: 0,
            bytes_per_row: Some(4 * texture_size.width),
            rows_per_image: Some(texture_size.height),
        },
        texture_size,
    );

    texture
}

impl eframe::App for FitsViewApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("FITS View");
            if let Some(meta) = &self.meta {
                ui.label(format!("Shape: {}x{}", meta.shape[0], meta.shape[1]));
                ui.label(format!("Data Type: {}", meta.dtype));
            } else {
                ui.label("Failed to fetch metadata. Is the server running?");
            }
            
            // Display viewport info
            ui.label(format!("Zoom: {:.2}x, Pan: ({:.1}, {:.1}), Rotation: {:.1}°", 
                self.viewport.zoom, 
                self.viewport.pan_offset.x, 
                self.viewport.pan_offset.y, 
                self.viewport.rotation_angle.to_degrees()));
            
            ui.separator();
            let (rect, response) = ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
            
            // Handle input
            self.handle_input(&response, ctx, frame);
            if let Some(_renderer) = &self.renderer {
                let viewport = self.viewport.clone();
                let meta = self.meta.clone();
                let render_rect = rect; // Capture rect for the closure
                let backend_url_clone = self.backend_url.clone();
                
                // Clone data for both closures
                let viewport_prepare = viewport.clone();
                let meta_prepare = meta.clone();
                let viewport_paint = viewport.clone();
                let meta_paint = meta.clone();
                
                let callback_fn = egui_wgpu::CallbackFn::new()
                    .prepare(move |device, queue, _encoder, resources| {
                        let resources: &mut Custom3dResources = resources.get_mut().unwrap();
                        
                        // Calculate render size and image size
                        let render_size = egui::Vec2::new(render_rect.width(), render_rect.height());
                        let image_size = if let Some(ref meta) = meta_prepare {
                            egui::Vec2::new(meta.shape[0] as f32, meta.shape[1] as f32)
                        } else {
                            egui::Vec2::new(1024.0, 1024.0) // Default size
                        };
                        
                        // Get visible tiles based on current viewport
                        let visible_tiles = viewport_prepare.get_visible_tiles(
                            render_size, 
                            image_size, 
                            resources.tile_manager.tile_size
                        );
                        
                        // Ensure all visible tiles are loaded
                        for tile_coord in &visible_tiles {
                            resources.tile_manager.get_or_create_tile(
                                tile_coord.clone(),
                                device,
                                queue,
                                &backend_url_clone,
                                &resources.texture_bind_group_layout,
                            );
                        }
                        
                        // Update uniform buffer with base viewport transform
                        let (vmin, vmax) = if let Some(ref meta) = meta_prepare {
                            (meta.min_val, meta.max_val)
                        } else {
                            (0.0, 255.0)
                        };
                        
                        // Get visible tiles for uniform buffer preparation
                        let visible_tiles = viewport_prepare.get_visible_tiles(
                            render_size, 
                            image_size, 
                            resources.tile_manager.tile_size
                        );
                        
                        // Create uniform data for each visible tile with proper alignment
                        let mut tile_index = 0;
                        for tile_coord in &visible_tiles {
                            if resources.tile_manager.tiles.contains_key(tile_coord) {
                                let tile_offset_x = tile_coord.x as f32 * resources.tile_manager.tile_size as f32;
                                // Flip Y coordinate: in image space, Y=0 is at top, but in graphics Y=0 is at bottom
                                let max_tiles_y = (image_size.y as u32 + resources.tile_manager.tile_size - 1) / resources.tile_manager.tile_size;
                                let flipped_y = (max_tiles_y - 1 - tile_coord.y) as f32;
                                let tile_offset_y = flipped_y * resources.tile_manager.tile_size as f32;
                                
                                let tile_uniforms = Uniforms {
                                    transform: viewport_prepare.to_transform_matrix(render_size, image_size),
                                    tile_offset: [tile_offset_x, tile_offset_y],
                                    vmin,
                                    vmax,
                                    _padding: [0.0, 0.0],
                                };
                                
                                // Write uniform data at 256-byte aligned offset
                                let offset = tile_index * 256;
                                queue.write_buffer(
                                    &resources.uniform_buffer,
                                    offset as u64,
                                    bytemuck::cast_slice(&[tile_uniforms]),
                                );
                                
                                tile_index += 1;
                            }
                        }
                        
                        Vec::new()
                    })
                    .paint(move |_info, render_pass, resources| {
                        let resources: &Custom3dResources = resources.get().unwrap();
                        render_pass.set_pipeline(&resources.pipeline);
                        render_pass.set_vertex_buffer(0, resources.vertex_buffer.slice(..));
                        render_pass.set_index_buffer(resources.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
                        
                        // Calculate render and image sizes
                        let render_size = egui::Vec2::new(render_rect.width(), render_rect.height());
                        let image_size = if let Some(ref meta) = meta_paint {
                            egui::Vec2::new(meta.shape[0] as f32, meta.shape[1] as f32)
                        } else {
                            egui::Vec2::new(1024.0, 1024.0)
                        };
                        
                        // Get visible tiles
                        let visible_tiles = viewport_paint.get_visible_tiles(
                            render_size, 
                            image_size, 
                            resources.tile_manager.tile_size
                        );
                        
                        // Render all visible tiles with proper positioning
                        let mut tile_index = 0;
                        for tile_coord in &visible_tiles {
                            if let Some(tile_data) = resources.tile_manager.tiles.get(tile_coord) {
                                // Calculate properly aligned offset (256-byte alignment required)
                                let uniform_offset = (tile_index * 256) as u32;
                                
                                render_pass.set_bind_group(0, &tile_data.bind_group, &[]);
                                render_pass.set_bind_group(1, &resources.uniform_bind_group, &[uniform_offset]);
                                render_pass.draw_indexed(0..resources.num_indices, 0, 0..1);
                                
                                tile_index += 1;
                            }
                        }
                        
                    });
                let callback = egui::PaintCallback {
                    rect,
                    callback: Arc::new(callback_fn),
                };
                ui.painter().add(callback);
            }
        });
    }
}

// Matrix multiplication helper function
fn multiply_matrices(a: &[[f32; 4]; 4], b: &[[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut result = [[0.0; 4]; 4];
    for i in 0..4 {
        for j in 0..4 {
            for k in 0..4 {
                result[i][j] += a[i][k] * b[k][j];
            }
        }
    }
    result
}

fn main() -> eframe::Result<()> {
    let args = Args::parse();
    let backend_url = args.backend_url.clone();
    
    let mut native_options = eframe::NativeOptions::default();
    native_options.renderer = eframe::Renderer::Wgpu;
    eframe::run_native(
        "FITS View",
        native_options,
        Box::new(move |cc| Box::new(FitsViewApp::new(cc, backend_url))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_viewport_default() {
        let viewport = Viewport::default();
        assert_eq!(viewport.zoom, 1.0);
        assert_eq!(viewport.pan_offset, egui::Vec2::ZERO);
        assert_eq!(viewport.rotation_angle, 0.0);
    }

    #[test]
    fn test_identity_transformation() {
        let viewport = Viewport::default();
        let render_size = egui::Vec2::new(800.0, 600.0);
        let image_size = egui::Vec2::new(400.0, 300.0);
        let matrix = viewport.to_transform_matrix(render_size, image_size);
        
        // With zoom=1.0, image should be rendered at actual pixel size
        let expected_scale_x = 400.0 * 2.0 / 800.0; // 1.0
        let expected_scale_y = 300.0 * 2.0 / 600.0; // 1.0
        
        assert_eq!(matrix[0], [expected_scale_x, 0.0, 0.0, 0.0]);
        assert_eq!(matrix[1], [0.0, expected_scale_y, 0.0, 0.0]);
        assert_eq!(matrix[2], [0.0, 0.0, 1.0, 0.0]);
        assert_eq!(matrix[3], [0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn test_pure_translation() {
        let mut viewport = Viewport::default();
        viewport.pan_offset = egui::Vec2::new(5.0, 10.0);
        
        let render_size = egui::Vec2::new(800.0, 600.0);
        let image_size = egui::Vec2::new(400.0, 300.0);
        let matrix = viewport.to_transform_matrix(render_size, image_size);
        
        // Translation should only affect the translation column
        assert_eq!(matrix[3], [5.0, 10.0, 0.0, 1.0]);
    }

    #[test]
    fn test_pure_scaling() {
        let mut viewport = Viewport::default();
        viewport.zoom = 2.0;
        
        let render_size = egui::Vec2::new(800.0, 600.0);
        let image_size = egui::Vec2::new(400.0, 300.0);
        let matrix = viewport.to_transform_matrix(render_size, image_size);
        
        // With 2x zoom, scaling should be doubled
        let expected_scale_x = 2.0 * 400.0 * 2.0 / 800.0; // 2.0
        let expected_scale_y = 2.0 * 300.0 * 2.0 / 600.0; // 2.0
        
        assert_eq!(matrix[0], [expected_scale_x, 0.0, 0.0, 0.0]);
        assert_eq!(matrix[1], [0.0, expected_scale_y, 0.0, 0.0]);
        assert_eq!(matrix[2], [0.0, 0.0, 1.0, 0.0]);
        assert_eq!(matrix[3], [0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn test_fixed_pixel_size() {
        let viewport = Viewport::default();
        
        // Same image size, different window sizes - image should appear same pixel size
        let image_size = egui::Vec2::new(256.0, 256.0);
        
        let matrix1 = viewport.to_transform_matrix(egui::Vec2::new(800.0, 600.0), image_size);
        let matrix2 = viewport.to_transform_matrix(egui::Vec2::new(1200.0, 900.0), image_size);
        
        // Both should render image at same pixel size (256x256)
        let scale1_x = matrix1[0][0];
        let scale1_y = matrix1[1][1];
        let scale2_x = matrix2[0][0];
        let scale2_y = matrix2[1][1];
        
        // Pixel size = scale * render_size / 2
        let pixel_size1_x = scale1_x * 800.0 / 2.0;
        let pixel_size1_y = scale1_y * 600.0 / 2.0;
        let pixel_size2_x = scale2_x * 1200.0 / 2.0;
        let pixel_size2_y = scale2_y * 900.0 / 2.0;
        
        assert!((pixel_size1_x - pixel_size2_x).abs() < 0.001);
        assert!((pixel_size1_y - pixel_size2_y).abs() < 0.001);
    }
}
