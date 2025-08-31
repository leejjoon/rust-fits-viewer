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
    Vertex { position: [-1.0, -1.0], tex_coords: [0.0, 1.0] }, // A
    Vertex { position: [1.0, -1.0], tex_coords: [1.0, 1.0] }, // B
    Vertex { position: [1.0, 1.0], tex_coords: [1.0, 0.0] }, // C
    Vertex { position: [-1.0, 1.0], tex_coords: [0.0, 0.0] }, // D
];

const INDICES: &[u16] = &[0, 1, 2, 0, 2, 3];

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    transform: [[f32; 4]; 4], // 4x4 transformation matrix
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
    
    fn to_transform_matrix(&self) -> [[f32; 4]; 4] {
        // Column-major 2D transformation matrix for WGSL
        // WGSL expects column-major matrices, so we need to transpose our row-major thinking
        let cos_r = self.rotation_angle.cos();
        let sin_r = self.rotation_angle.sin();
        
        // Column-major matrix: each inner array is a column
        // Column 0: [scale*cos, scale*sin, 0, 0]
        // Column 1: [-scale*sin, scale*cos, 0, 0] 
        // Column 2: [0, 0, 1, 0]
        // Column 3: [pan_x, pan_y, 0, 1]
        [
            [self.zoom * cos_r, self.zoom * sin_r, 0.0, 0.0],      // Column 0
            [-self.zoom * sin_r, self.zoom * cos_r, 0.0, 0.0],     // Column 1
            [0.0, 0.0, 1.0, 0.0],                                   // Column 2
            [self.pan_offset.x, self.pan_offset.y, 0.0, 1.0],      // Column 3 (translation)
        ]
    }
}


struct Custom3dResources {
    pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    num_indices: u32,
    diffuse_bind_group: wgpu::BindGroup,
    uniform_bind_group: wgpu::BindGroup,
    uniform_buffer: wgpu::Buffer,
}

struct Custom3d {
    backend_url: String,
}

impl Custom3d {
    fn new(wgpu_render_state: &egui_wgpu::RenderState, backend_url: String, meta: Option<&MetaResponse>) -> Self {
        let device = &wgpu_render_state.device;
        let queue = &wgpu_render_state.queue;

        // Get normalization values from metadata
        let (vmin, vmax) = if let Some(meta) = meta {
            (meta.min_val, meta.max_val)
        } else {
            (0.0, 255.0) // Default fallback values
        };

        println!("Using normalization: vmin={}, vmax={}", vmin, vmax);

        // Fetch and create FITS tile texture
        let diffuse_texture = match fetch_tile_data(&backend_url, 0, 0, 0) {
            Ok(tile_bytes) => {
                match parse_raw_tile(&tile_bytes) {
                    Ok(tile) => {
                        println!("Successfully parsed tile: {}x{}", tile.header.width, tile.header.height);
                        upload_r32float_texture(device, queue, &tile)
                    }
                    Err(e) => {
                        eprintln!("Failed to parse tile: {}", e);
                        // Fallback to test texture
                        create_test_texture(device, queue)
                    }
                }
            }
            Err(e) => {
                eprintln!("Failed to fetch tile: {}", e);
                // Fallback to test texture
                create_test_texture(device, queue)
            }
        };

        let diffuse_texture_view = diffuse_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let diffuse_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

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
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
            label: Some("uniform_bind_group_layout"),
        });

        let diffuse_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &texture_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&diffuse_texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&diffuse_sampler),
                },
            ],
            label: Some("diffuse_bind_group"),
        });

        let identity_matrix = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Uniform Buffer"),
            contents: bytemuck::cast_slice(&[Uniforms { 
                transform: identity_matrix,
                vmin, 
                vmax, 
                _padding: [0.0, 0.0] 
            }]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &uniform_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
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
                diffuse_bind_group,
                uniform_bind_group,
                uniform_buffer,
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
        Self { meta, renderer, backend_url, viewport: Viewport::default() }
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
        
        // Mouse scroll for zooming
        if let Some(_hover_pos) = response.hover_pos() {
            let scroll_delta = ctx.input(|i| i.scroll_delta.y);
            if scroll_delta != 0.0 {
                if ctx.input(|i| i.modifiers.ctrl) {
                    // Ctrl + scroll for rotation
                    self.viewport.rotation_angle += scroll_delta * 0.01;
                } else {
                    // Regular scroll for zoom
                    let zoom_factor = 1.0 + scroll_delta * 0.001;
                    self.viewport.zoom = (self.viewport.zoom * zoom_factor).clamp(0.1, 10.0);
                }
            }
        }
        
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
                    Err(e) => {
                        eprintln!("Failed to parse meta response: {}", e);
                        None
                    }
                }
            } else {
                eprintln!("Meta request failed with status: {}", response.status());
                None
            }
        }
        Err(e) => {
            eprintln!("Failed to fetch meta: {}", e);
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
                    Err(e) => Err(format!("Failed to read tile response: {}", e)),
                }
            } else {
                Err(format!("Tile request failed with status: {}", response.status()))
            }
        }
        Err(e) => Err(format!("Failed to fetch tile: {}", e)),
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
                let callback_fn = egui_wgpu::CallbackFn::new()
                    .prepare(move |_device, queue, _encoder, resources| {
                        let resources: &Custom3dResources = resources.get().unwrap();
                        
                        // Update uniform buffer with current viewport transform
                        let (vmin, vmax) = if let Some(ref meta) = meta {
                            (meta.min_val, meta.max_val)
                        } else {
                            (0.0, 255.0)
                        };
                        
                        let uniforms = Uniforms {
                            transform: viewport.to_transform_matrix(),
                            vmin,
                            vmax,
                            _padding: [0.0, 0.0],
                        };
                        
                        queue.write_buffer(
                            &resources.uniform_buffer,
                            0,
                            bytemuck::cast_slice(&[uniforms]),
                        );
                        
                        Vec::new()
                    })
                    .paint(|_info, render_pass, resources| {
                        let resources: &Custom3dResources = resources.get().unwrap();
                        render_pass.set_pipeline(&resources.pipeline);
                        render_pass.set_bind_group(0, &resources.diffuse_bind_group, &[]);
                        render_pass.set_bind_group(1, &resources.uniform_bind_group, &[]);
                        render_pass.set_vertex_buffer(0, resources.vertex_buffer.slice(..));
                        render_pass.set_index_buffer(resources.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
                        render_pass.draw_indexed(0..resources.num_indices, 0, 0..1);
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
        let matrix = viewport.to_transform_matrix();
        
        // Identity matrix should be:
        // [1, 0, 0, 0]
        // [0, 1, 0, 0]
        // [0, 0, 1, 0]
        // [0, 0, 0, 1]
        assert_eq!(matrix[0], [1.0, 0.0, 0.0, 0.0]);
        assert_eq!(matrix[1], [0.0, 1.0, 0.0, 0.0]);
        assert_eq!(matrix[2], [0.0, 0.0, 1.0, 0.0]);
        assert_eq!(matrix[3], [0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn test_pure_translation() {
        let mut viewport = Viewport::default();
        viewport.pan_offset = egui::Vec2::new(5.0, 10.0);
        
        let matrix = viewport.to_transform_matrix();
        
        // Translation should only affect the translation column
        assert_eq!(matrix[0], [1.0, 0.0, 0.0, 5.0]);
        assert_eq!(matrix[1], [0.0, 1.0, 0.0, 10.0]);
        assert_eq!(matrix[2], [0.0, 0.0, 1.0, 0.0]);
        assert_eq!(matrix[3], [0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn test_pure_scaling() {
        let mut viewport = Viewport::default();
        viewport.zoom = 2.0;
        
        let matrix = viewport.to_transform_matrix();
        
        // Scaling should only affect diagonal elements
        assert_eq!(matrix[0], [2.0, 0.0, 0.0, 0.0]);
        assert_eq!(matrix[1], [0.0, 2.0, 0.0, 0.0]);
        assert_eq!(matrix[2], [0.0, 0.0, 1.0, 0.0]);
        assert_eq!(matrix[3], [0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn test_no_rotation_components() {
        let mut viewport = Viewport::default();
        viewport.rotation_angle = 45.0; // This should NOT affect the matrix
        viewport.zoom = 2.0;
        viewport.pan_offset = egui::Vec2::new(1.0, 1.0);
        
        let matrix = viewport.to_transform_matrix();
        
        // Off-diagonal elements should be zero (no rotation)
        assert_eq!(matrix[0][1], 0.0); // No Y component in X transformation
        assert_eq!(matrix[1][0], 0.0); // No X component in Y transformation
        
        // Should be pure scale + translate
        assert_eq!(matrix[0], [2.0, 0.0, 0.0, 1.0]);
        assert_eq!(matrix[1], [0.0, 2.0, 0.0, 1.0]);
    }

    #[test]
    fn test_matrix_determinant() {
        let mut viewport = Viewport::default();
        viewport.zoom = 2.0;
        
        let matrix = viewport.to_transform_matrix();
        
        // For a 2D transformation matrix [a b tx; c d ty; 0 0 1],
        // determinant = a*d - b*c
        let det = matrix[0][0] * matrix[1][1] - matrix[0][1] * matrix[1][0];
        
        // Determinant should equal zoom^2 for pure scaling
        assert_eq!(det, 4.0);
    }
}
