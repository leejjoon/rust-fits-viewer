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
use fits_view_client::{
    MetaResponse, Viewport, 
    RenderSize, ImageSize, 
    create_image_to_ndc_matrix, calculate_max_lod, calculate_lod, calculate_visible_tiles,
    tile_manager::{TileManager, TileCoord},
};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Backend server URL
    #[arg(long, default_value = "http://127.0.0.1:8001")]
    backend_url: String,
    
    /// Initial pan offset X in NDC space (for testing) - positive = right
    #[arg(long, default_value = "0.0")]
    initial_pan_x: f32,
    
    /// Initial pan offset Y in NDC space (for testing) - positive = up
    #[arg(long, default_value = "0.0")]
    initial_pan_y: f32,
}

// Simple viewport struct for main.rs
#[derive(Debug, Clone, Copy)]
struct SimpleViewport {
    pub zoom: f32,
    pub pan_offset: egui::Vec2,
    pub rotation_angle: f32,
}

impl Default for SimpleViewport {
    fn default() -> Self {
        let render_size = RenderSize { width: 800.0, height: 600.0 };
        let image_size = ImageSize { width: 1024.0, height: 1024.0 };
        // Calculate zoom to fit image in viewport - increase zoom to make tiles more visible
        let fit_zoom_x = render_size.width / image_size.width;
        let fit_zoom_y = render_size.height / image_size.height;
        let fit_zoom = fit_zoom_x.min(fit_zoom_y) * 1.5; // Increase zoom to 150%
        
        let simple_viewport = SimpleViewport {
            zoom: fit_zoom,
            pan_offset: egui::Vec2::new(0.0, 0.0),
            rotation_angle: 0.0,
        };
        simple_viewport
    }
}

impl SimpleViewport {
    pub fn fit_to_window(&mut self, window_size: egui::Vec2, image_size: egui::Vec2) {
        let scale_x = window_size.x / image_size.x;
        let scale_y = window_size.y / image_size.y;
        self.zoom = scale_x.min(scale_y) * 1.5; // Increase zoom to 150%
        self.pan_offset = egui::Vec2::ZERO;
        self.rotation_angle = 0.0;
    }
    
    pub fn set_pan_offset(&mut self, pan_offset: egui::Vec2) {
        self.pan_offset = pan_offset;
    }
    
    pub fn set_zoom(&mut self, zoom: f32) {
        self.zoom = zoom;
    }
    
    pub fn reset(&mut self) {
        *self = Self::default();
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
    transform: [[f32; 4]; 4], // 4x4 transformation matrix (64 bytes)
    tile_offset: [f32; 2], // Tile position offset in image coordinates (8 bytes)
    vmin: f32,
    vmax: f32,
    tile_size: f32,
    _padding1: f32,
    _padding2: [f32; 2],
    _padding3: [f32; 4],
}


struct TileData {
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    coord: TileCoord,
}

struct GpuTileManager {
    tiles: std::collections::HashMap<TileCoord, TileData>,
    tile_size: u32,
    image_size: [u32; 2],
}

// Use EguiViewport from lib.rs instead of local implementation


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

struct Custom3d {
    backend_url: String,
}

impl GpuTileManager {
    fn new(tile_size: u32, image_size: [u32; 2]) -> Self {
        Self {
            tiles: std::collections::HashMap::new(),
            tile_size,
            image_size,
        }
    }
    
    fn get_or_create_tile(&mut self, coord: TileCoord, device: &wgpu::Device, queue: &wgpu::Queue, backend_url: &str, texture_bind_group_layout: &wgpu::BindGroupLayout) -> Option<&TileData> {
        if self.tiles.contains_key(&coord) {
            return self.tiles.get(&coord);
        }
        
        // Fetch real tile data from server
        let f32_data = match fetch_tile_data(backend_url, coord.lod, coord.x, coord.y) {
            Ok(raw_bytes) => {
                match parse_raw_tile_data(&raw_bytes) {
                    Ok(data) => {
                        println!("✅ Fetched real tile data ({}, {}, {}) - {} pixels", coord.lod, coord.x, coord.y, data.len());
                        data
                    },
                    Err(e) => {
                        println!("⚠️ Failed to parse tile ({}, {}, {}): {}, using fallback", coord.lod, coord.x, coord.y, e);
                        self.generate_fallback_tile()
                    }
                }
            },
            Err(e) => {
                println!("⚠️ Failed to fetch tile ({}, {}, {}): {}, using fallback", coord.lod, coord.x, coord.y, e);
                self.generate_fallback_tile()
            }
        };
        
        let texture_size = wgpu::Extent3d {
            width: self.tile_size,
            height: self.tile_size,
            depth_or_array_layers: 1,
        };
        
        // Create texture directly using wgpu
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

        // Upload the f32 data to the texture
        let bytes_per_row = std::num::NonZeroU32::new(4 * self.tile_size).unwrap();
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(&f32_data),
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row.into()),
                rows_per_image: Some(std::num::NonZeroU32::new(self.tile_size).unwrap().into()),
            },
            texture_size,
        );
        
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
            label: Some(&format!("checkerboard_tile_{}_{}", coord.x, coord.y)),
        });
        
        let tile_data = TileData {
            texture,
            bind_group,
            coord: coord.clone(),
        };
        
        self.tiles.insert(coord.clone(), tile_data);
        self.tiles.get(&coord)
    }
    
    fn generate_fallback_tile(&self) -> Vec<f32> {
        let mut f32_data = vec![0.0f32; (self.tile_size * self.tile_size) as usize];
        
        // Create checkerboard pattern as fallback
        for y in 0..self.tile_size {
            for x in 0..self.tile_size {
                let idx = (y * self.tile_size + x) as usize;
                let is_white = (x + y) % 2 == 0;
                f32_data[idx] = if is_white { 255.0 } else { 0.0 };
            }
        }
        
        f32_data
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
            (256, [1024, 1024])
        };

        let tile_manager = GpuTileManager::new(tile_size, image_size);

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

        let uniforms_size = std::mem::size_of::<Uniforms>();
        
        let uniform_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(uniforms_size as u64),
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
                        size: wgpu::BufferSize::new(uniforms_size as u64),
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
    viewport: SimpleViewport,
}

impl Default for FitsViewApp {
    fn default() -> Self {
        Self {
            meta: None,
            renderer: None,
            backend_url: "http://127.0.0.1:8001".to_string(),
            viewport: SimpleViewport::default(),
        }
    }
}

impl FitsViewApp {
    fn new(cc: &eframe::CreationContext<'_>, backend_url: String, initial_pan: Option<egui::Vec2>) -> Self {
        let meta = fetch_meta(&backend_url);
        let renderer = {
            let wgpu_render_state = cc.wgpu_render_state.as_ref().expect("wgpu backend");
            Some(Custom3d::new(wgpu_render_state, backend_url.clone(), meta.as_ref()))
        };
        
        // Initialize viewport
        let mut viewport = SimpleViewport::default();
        if let Some(meta) = &meta {
            // Use a reasonable default window size for initial centering
            let window_size = egui::Vec2::new(800.0, 600.0);
            let image_size = egui::Vec2::new(meta.shape[0] as f32, meta.shape[1] as f32);
            viewport.fit_to_window(window_size, image_size);
            
            // Apply initial pan offset if provided (for testing)
            if let Some(pan) = initial_pan {
                viewport.set_pan_offset(pan);
                println!("🎯 Applied initial pan offset: {:?}", pan);
            }
            
            println!("🎯 Initial viewport setup:");
            println!("   Window size: {:?}", window_size);
            println!("   Image size: {:?}", image_size);
            println!("   Initial zoom: {:.3}", viewport.zoom);
            println!("   Initial pan: {:?}", viewport.pan_offset);
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
            let current_pan = self.viewport.pan_offset;
            self.viewport.set_pan_offset(current_pan + delta * sensitivity);
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
                        let current_rotation = self.viewport.rotation_angle;
                        self.viewport.rotation_angle = current_rotation + scroll_delta * 0.005;
                    } else {
                        // Regular scroll for zoom - zoom around mouse position
                        let zoom_factor = 1.0 + scroll_delta * 0.001;
                        let current_zoom = self.viewport.zoom;
                        let new_zoom = (current_zoom * zoom_factor).clamp(0.1, 10.0);
                        
                        if new_zoom != current_zoom {
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
                            let current_pan = self.viewport.pan_offset;
                            let image_x = (mouse_ndc_x - current_pan.x) / (current_zoom * pixels_to_ndc_x);
                            let image_y = (mouse_ndc_y - current_pan.y) / (current_zoom * pixels_to_ndc_y);
                            
                            // Apply new zoom
                            self.viewport.set_zoom(new_zoom);
                            
                            // Calculate new pan offset to keep the same image point under the mouse
                            let new_ndc_x = image_x * (new_zoom * pixels_to_ndc_x);
                            let new_ndc_y = image_y * (new_zoom * pixels_to_ndc_y);
                            
                            let new_pan = egui::Vec2::new(
                                mouse_ndc_x - new_ndc_x,
                                mouse_ndc_y - new_ndc_y
                            );
                            self.viewport.set_pan_offset(new_pan);
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

fn parse_raw_tile_data(raw_bytes: &[u8]) -> Result<Vec<f32>, String> {
    use crate::raw_header::{RawTileHeader, DT_F32, ENDIAN_LITTLE};
    
    // Parse the header
    let (header, payload) = RawTileHeader::from_prefix(raw_bytes)
        .map_err(|e| format!("Failed to parse header: {}", e))?;
    
    // Validate header
    if header.dtype_code != DT_F32 {
        return Err(format!("Unsupported data type: {}", header.dtype_code));
    }
    
    if header.endianness != ENDIAN_LITTLE {
        return Err(format!("Unsupported endianness: {}", header.endianness));
    }
    
    // Calculate expected payload size
    let expected_size = (header.width * header.height * 4) as usize; // 4 bytes per f32
    if payload.len() != expected_size {
        return Err(format!("Payload size mismatch: expected {}, got {}", expected_size, payload.len()));
    }
    
    // Convert bytes to f32 array
    let f32_data: Vec<f32> = payload
        .chunks_exact(4)
        .map(|chunk| {
            let bytes = [chunk[0], chunk[1], chunk[2], chunk[3]];
            f32::from_le_bytes(bytes)
        })
        .collect();
    
    println!("📊 Parsed tile: {}x{}, {} pixels, range: {:.2}..{:.2}", 
             header.width, header.height, f32_data.len(),
             f32_data.iter().fold(f32::INFINITY, |a, &b| a.min(b)),
             f32_data.iter().fold(f32::NEG_INFINITY, |a, &b| a.max(b)));
    
    Ok(f32_data)
}

fn fetch_tile_data(backend_url: &str, z: u32, x: u32, y: u32) -> Result<Vec<u8>, String> {
    let url = format!("{}/tile/{}/{}/{}?format=raw&dtype=float32", backend_url, z, x, y);
    println!("🌐 Requesting: {}", url);
    match reqwest::blocking::get(&url) {
        Ok(response) => {
            println!("📡 Response status: {}", response.status());
            if response.status().is_success() {
                match response.bytes() {
                    Ok(bytes) => {
                        println!("📦 Received {} bytes", bytes.len());
                        Ok(bytes.to_vec())
                    },
                    Err(e) => Err(format!("Failed to read tile response: {}", e)),
                }
            } else {
                Err(format!("Tile request failed with status: {}", response.status()))
            }
        }
        Err(e) => {
            Err(format!("Failed to fetch tile: {}", e))
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
                ui.label(format!("Data Type: {}", "float32"));
            } else {
                ui.label("Failed to fetch metadata. Is the server running?");
            }
            
            // Display viewport info including LOD
            let current_lod = if let Some(meta) = &self.meta {
                let image_size = ImageSize { 
                    width: meta.shape[0] as f32, 
                    height: meta.shape[1] as f32 
                };
                let max_lod = calculate_max_lod(image_size, meta.tile_size);
                calculate_lod(self.viewport.zoom, max_lod)
            } else {
                0
            };
            
            ui.label(format!("Zoom: {:.2}x, Pan: ({:.1}, {:.1}), Rotation: {:.1}°, LOD: {}", 
                self.viewport.zoom, 
                self.viewport.pan_offset.x, 
                self.viewport.pan_offset.y, 
                self.viewport.rotation_angle.to_degrees(),
                current_lod));
            
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
                        
                        // Calculate transformation matrix first
                        let core_viewport = Viewport {
                            zoom: viewport_prepare.zoom,
                            pan_offset: [viewport_prepare.pan_offset.x, viewport_prepare.pan_offset.y],
                            rotation_angle: viewport_prepare.rotation_angle,
                        };
                        let image_size_for_transform = if let Some(ref meta) = meta_prepare {
                            ImageSize { width: meta.shape[0] as f32, height: meta.shape[1] as f32 }
                        } else {
                            ImageSize { width: 1024.0, height: 1024.0 }
                        };
                        
                        let transform_matrix = create_image_to_ndc_matrix(
                            &core_viewport,
                            RenderSize { width: render_size.x, height: render_size.y },
                            image_size_for_transform,
                        );
                        
                        // Get visible tiles and ensure they are loaded
                        let render_size_spec = RenderSize { width: render_size.x, height: render_size.y };
                        let image_size_spec = ImageSize { width: image_size.x, height: image_size.y };
                        let max_lod = calculate_max_lod(image_size_spec, resources.tile_manager.tile_size);
                        let visible_tiles = calculate_visible_tiles(&core_viewport, render_size_spec, image_size_spec, resources.tile_manager.tile_size, max_lod);
                        
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
                        
                        // Calculate transformation matrix using new coordinate transform module
                        let core_viewport = Viewport {
                            zoom: viewport_prepare.zoom,
                            pan_offset: [viewport_prepare.pan_offset.x, viewport_prepare.pan_offset.y],
                            rotation_angle: viewport_prepare.rotation_angle,
                        };
                        let image_size_for_transform = if let Some(ref meta) = meta_prepare {
                            ImageSize { width: meta.shape[0] as f32, height: meta.shape[1] as f32 }
                        } else {
                            ImageSize { width: 1024.0, height: 1024.0 }
                        };
                        
                        let transform_matrix = create_image_to_ndc_matrix(
                            &core_viewport,
                            RenderSize { width: render_size.x, height: render_size.y },
                            image_size_for_transform,
                        );
                        
                        // Debug output matching fail01.json format
                        println!("=== DEBUG OUTPUT (matching fail01.json) ===");
                        println!("Input:");
                        println!("  render_size: [{:.0}, {:.0}]", render_size.x, render_size.y);
                        println!("  image_size: [{:.0}, {:.0}]", image_size_for_transform.width, image_size_for_transform.height);
                        println!("  zoom: {:.3}", core_viewport.zoom);
                        println!("  pan_offset: [{:.1}, {:.1}]", core_viewport.pan_offset[0], core_viewport.pan_offset[1]);
                        println!("  tile_size: {}", resources.tile_manager.tile_size);
                        
                        // Calculate and display LOD
                        let selected_lod = visible_tiles.first().map(|t| t.lod).unwrap_or(0);
                        println!("Output:");
                        println!("  lod: {}", selected_lod);
                        
                        // Calculate padded area
                        let padding = 0.5 * resources.tile_manager.tile_size as f32;
                        let padded_area = [
                            -padding,
                            -padding,
                            render_size.x + 2.0 * padding,
                            render_size.y + 2.0 * padding,
                        ];
                        println!("  padded_area: [{:.1}, {:.1}, {:.1}, {:.1}]", 
                            padded_area[0], padded_area[1], padded_area[2], padded_area[3]);
                        
                        // Display visible tiles as visibility mask
                        let max_tiles_x = (image_size_for_transform.width / resources.tile_manager.tile_size as f32).ceil() as u32;
                        let max_tiles_y = (image_size_for_transform.height / resources.tile_manager.tile_size as f32).ceil() as u32;
                        
                        // Create visibility mask for the selected LOD
                        let mut visibility_mask = vec![vec!['0'; (max_tiles_x >> selected_lod) as usize]; (max_tiles_y >> selected_lod) as usize];
                        for tile in &visible_tiles {
                            if tile.lod == selected_lod && tile.x < (max_tiles_x >> selected_lod) && tile.y < (max_tiles_y >> selected_lod) {
                                visibility_mask[tile.y as usize][tile.x as usize] = '1';
                            }
                        }
                        
                        print!("  visibility_mask: [");
                        for (i, row) in visibility_mask.iter().rev().enumerate() {
                            if i > 0 { print!(", "); }
                            print!("\"{}\"", row.iter().collect::<String>());
                        }
                        println!("]");
                        
                        // Calculate image screen extent
                        let image_center_x = image_size_for_transform.width / 2.0;
                        let image_center_y = image_size_for_transform.height / 2.0;
                        let render_center_x = render_size.x / 2.0;
                        let render_center_y = render_size.y / 2.0;
                        
                        let zoom_inv = 1.0 / core_viewport.zoom;
                        let pan_in_image_x = -core_viewport.pan_offset[0] * zoom_inv;
                        let pan_in_image_y = core_viewport.pan_offset[1] * zoom_inv;
                        
                        let center_x = image_center_x + pan_in_image_x;
                        let center_y = image_center_y + pan_in_image_y;
                        
                        let image_min_x = ((0.0 - center_x) * core_viewport.zoom + render_center_x).round() as i32;
                        let image_min_y = ((0.0 - center_y) * core_viewport.zoom + render_center_y).round() as i32;
                        let image_max_x = ((image_size_for_transform.width - center_x) * core_viewport.zoom + render_center_x).round() as i32;
                        let image_max_y = ((image_size_for_transform.height - center_y) * core_viewport.zoom + render_center_y).round() as i32;
                        
                        println!("  image_screen_extent:");
                        println!("    min: [{}, {}]", image_min_x, image_min_y);
                        println!("    max: [{}, {}]", image_max_x, image_max_y);
                        
                        // Additional debug info for rendering issues
                        println!("🔧 Transform matrix: {:?}", transform_matrix);
                        println!("🎯 vmin: {:.3}, vmax: {:.3}", vmin, vmax);
                        println!("📍 Visible tiles: {:?}", visible_tiles);
                        
                        // Check if tiles are positioned within viewport
                        for tile_coord in &visible_tiles {
                            // Account for LOD scaling in debug output
                            let lod_scale = 2_u32.pow(tile_coord.lod) as f32;
                            let effective_tile_size = resources.tile_manager.tile_size as f32 * lod_scale;
                            let tile_offset_x = tile_coord.x as f32 * effective_tile_size;
                            let tile_offset_y = tile_coord.y as f32 * effective_tile_size;
                            
                            // Calculate where this tile will appear in NDC space
                            let tile_corners = [
                                [tile_offset_x, tile_offset_y, 0.0, 1.0],
                                [tile_offset_x + effective_tile_size, tile_offset_y, 0.0, 1.0],
                                [tile_offset_x, tile_offset_y + effective_tile_size, 0.0, 1.0],
                                [tile_offset_x + effective_tile_size, tile_offset_y + effective_tile_size, 0.0, 1.0],
                            ];
                            
                            println!("🔲 Tile ({},{},{}) offset: ({:.1}, {:.1})", tile_coord.lod, tile_coord.x, tile_coord.y, tile_offset_x, tile_offset_y);
                            
                            for (i, corner) in tile_corners.iter().enumerate() {
                                let transformed = [
                                    transform_matrix[0][0] * corner[0] + transform_matrix[0][1] * corner[1] + transform_matrix[0][2] * corner[2] + transform_matrix[0][3] * corner[3],
                                    transform_matrix[1][0] * corner[0] + transform_matrix[1][1] * corner[1] + transform_matrix[1][2] * corner[2] + transform_matrix[1][3] * corner[3],
                                ];
                                println!("   Corner {}: NDC ({:.3}, {:.3})", i, transformed[0], transformed[1]);
                            }
                        }
                        
                        println!("=== END DEBUG OUTPUT ===");
                        
                        // Remove duplicate visible tiles calculation - already done above
                        
                        // Create uniform data for each visible tile with proper alignment
                        let mut tile_index = 0;
                        for tile_coord in &visible_tiles {
                            if resources.tile_manager.tiles.contains_key(tile_coord) {
                                // Account for LOD scaling: at LOD n, each tile represents 2^n times larger area
                                let lod_scale = 2_u32.pow(tile_coord.lod) as f32;
                                let effective_tile_size = resources.tile_manager.tile_size as f32 * lod_scale;
                                let tile_offset_x = tile_coord.x as f32 * effective_tile_size;
                                // Direct Y coordinate - no flipping needed as coordinate_transform handles this
                                let tile_offset_y = tile_coord.y as f32 * effective_tile_size;
                                
                                let tile_uniforms = Uniforms {
                                    transform: transform_matrix,
                                    tile_offset: [tile_offset_x, tile_offset_y],
                                    vmin,
                                    vmax,
                                    tile_size: effective_tile_size,
                                    _padding1: 0.0,
                                    _padding2: [0.0, 0.0],
                                    _padding3: [0.0, 0.0, 0.0, 0.0],
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
                        let paint_viewport = Viewport {
                            zoom: viewport_paint.zoom,
                            pan_offset: [viewport_paint.pan_offset.x, viewport_paint.pan_offset.y],
                            rotation_angle: viewport_paint.rotation_angle,
                        };
                        let render_size_spec = RenderSize { width: render_size.x, height: render_size.y };
                        let image_size_spec = ImageSize { width: image_size.x, height: image_size.y };
                        let max_lod = calculate_max_lod(image_size_spec, resources.tile_manager.tile_size);
                        let visible_tiles = calculate_visible_tiles(&paint_viewport, render_size_spec, image_size_spec, resources.tile_manager.tile_size, max_lod);
                        
                        // Render all visible tiles
                        let mut tile_index = 0;
                        let mut rendered_tiles = 0;
                        for tile_coord in &visible_tiles {
                            if let Some(tile_data) = resources.tile_manager.tiles.get(tile_coord) {
                                // Calculate properly aligned offset (256-byte alignment required)
                                let uniform_offset = (tile_index * 256) as u32;
                                
                                render_pass.set_bind_group(0, &tile_data.bind_group, &[]);
                                render_pass.set_bind_group(1, &resources.uniform_bind_group, &[uniform_offset]);
                                render_pass.draw_indexed(0..resources.num_indices, 0, 0..1);
                                
                                tile_index += 1;
                                rendered_tiles += 1;
                            }
                        }
                        
                        if rendered_tiles == 0 && !visible_tiles.is_empty() {
                            println!("⚠️  No tiles rendered despite {} visible tiles", visible_tiles.len());
                            println!("   Available tiles in manager: {}", resources.tile_manager.tiles.len());
                            for (coord, _) in &resources.tile_manager.tiles {
                                println!("   - Available: ({}, {}, {})", coord.lod, coord.x, coord.y);
                            }
                            for coord in &visible_tiles {
                                println!("   - Requested: ({}, {}, {})", coord.lod, coord.x, coord.y);
                            }
                        } else if rendered_tiles > 0 {
                            println!("🎨 Rendered {} tiles", rendered_tiles);
                        }
                        
                    });
                let callback = egui::PaintCallback {
                    rect,
                    callback: Arc::new(callback_fn),
                };
                ui.painter().add(callback);
                
                // Draw debug overlay showing image extent
                self.draw_debug_overlay(ui, rect);
            }
        });
    }
    
}

impl FitsViewApp {
    fn draw_debug_overlay(&self, ui: &mut egui::Ui, rect: egui::Rect) {
        if let Some(meta) = &self.meta {
            let render_size = RenderSize { 
                width: rect.width(), 
                height: rect.height() 
            };
            let image_size = ImageSize { 
                width: meta.shape[0] as f32, 
                height: meta.shape[1] as f32 
            };
            
            // Create viewport for coordinate transformation
            let viewport = Viewport {
                zoom: self.viewport.zoom,
                pan_offset: [self.viewport.pan_offset.x, self.viewport.pan_offset.y],
                rotation_angle: self.viewport.rotation_angle,
            };
            
            // Get transformation matrix
            let transform_matrix = create_image_to_ndc_matrix(
                &viewport, render_size, image_size
            );
            
            // Get visible tiles to show their actual boundaries (full boundary overlay)
            let visible_tiles = calculate_visible_tiles(
                &viewport,
                render_size,
                image_size,
                meta.tile_size,
                calculate_max_lod(image_size, meta.tile_size)
            );
            
            let painter = ui.painter();
            
            // Draw each visible tile boundary (show all tiles in boundary)
            for tile_coord in &visible_tiles {
                // Account for LOD scaling: at LOD n, each tile represents 2^n times larger area
                let lod_scale = 2_u32.pow(tile_coord.lod) as f32;
                let effective_tile_size = meta.tile_size as f32 * lod_scale;
                let tile_offset_x = tile_coord.x as f32 * effective_tile_size;
                let tile_offset_y = tile_coord.y as f32 * effective_tile_size;
                
                // Match exactly what the shader does:
                // 1. Unit quad vertices [0,1] scaled to tile_size + tile_offset
                // 2. Apply transformation matrix
                let unit_quad = [
                    [0.0, 0.0],  // Bottom-left
                    [1.0, 0.0],  // Bottom-right
                    [0.0, 1.0],  // Top-left
                    [1.0, 1.0],  // Top-right
                ];
                
                let mut screen_corners = Vec::new();
                for vertex in &unit_quad {
                    // Scale unit quad to effective tile size and add offset (same as shader line 32)
                    let tile_pos_x = vertex[0] * effective_tile_size + tile_offset_x;
                    let tile_pos_y = vertex[1] * effective_tile_size + tile_offset_y;
                    
                    // Apply transformation matrix (same as shader line 35)
                    // WGSL uses column-major matrix multiplication: transform * vec4(tile_pos, 0.0, 1.0)
                    let ndc_x = transform_matrix[0][0] * tile_pos_x + transform_matrix[1][0] * tile_pos_y + transform_matrix[2][0] * 0.0 + transform_matrix[3][0] * 1.0;
                    let ndc_y = transform_matrix[0][1] * tile_pos_x + transform_matrix[1][1] * tile_pos_y + transform_matrix[2][1] * 0.0 + transform_matrix[3][1] * 1.0;
                    
                    // Convert NDC to screen coordinates
                    let screen_x = rect.min.x + (ndc_x + 1.0) * rect.width() * 0.5;
                    let screen_y = rect.min.y + (-ndc_y + 1.0) * rect.height() * 0.5;
                    screen_corners.push(egui::Pos2::new(screen_x, screen_y));
                }
                
                // Draw tile boundary
                let stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(255, 100, 100));
                painter.line_segment([screen_corners[0], screen_corners[1]], stroke); // Top edge
                painter.line_segment([screen_corners[1], screen_corners[3]], stroke); // Right edge  
                painter.line_segment([screen_corners[3], screen_corners[2]], stroke); // Bottom edge
                painter.line_segment([screen_corners[2], screen_corners[0]], stroke); // Left edge
                
                // Draw corner markers
                for corner in &screen_corners {
                    painter.circle_filled(*corner, 2.0, egui::Color32::YELLOW);
                }
            }
            
            // Show current viewport info including LOD
            let current_lod = calculate_lod(self.viewport.zoom, calculate_max_lod(image_size, meta.tile_size));
            let info_text = format!(
                "Zoom: {:.3}\nPan: ({:.3}, {:.3})\nRotation: {:.3}°\nLOD: {}\nTiles: {}",
                self.viewport.zoom,
                self.viewport.pan_offset.x,
                self.viewport.pan_offset.y,
                self.viewport.rotation_angle.to_degrees(),
                current_lod,
                visible_tiles.len()
            );
            
            painter.text(
                rect.min + egui::Vec2::new(10.0, 10.0),
                egui::Align2::LEFT_TOP,
                info_text,
                egui::FontId::default(),
                egui::Color32::WHITE,
            );
        }
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
    let initial_pan = egui::Vec2::new(args.initial_pan_x, args.initial_pan_y);
    
    let mut native_options = eframe::NativeOptions::default();
    native_options.renderer = eframe::Renderer::Wgpu;
    eframe::run_native(
        "FITS View",
        native_options,
        Box::new(move |cc| Box::new(FitsViewApp::new(cc, backend_url, Some(initial_pan)))),
    )
}

#[cfg(test_disabled)]
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
        let visible_tiles = viewport.get_visible_tiles(render_size, image_size, 256);
        
        // Debug output for tile visibility
        if visible_tiles.len() != 4 {
            println!("🔍 Visible tiles: {} (loaded: 4)", visible_tiles.len());
            println!("   Viewport: zoom={:.3}, pan={:?}", viewport.zoom, viewport.pan_offset);
        }
        
        // Load tiles that aren't already loaded
        for tile_coord in &visible_tiles {
            // Do nothing
        }
        
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
