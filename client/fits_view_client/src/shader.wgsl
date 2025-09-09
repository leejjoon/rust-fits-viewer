// Vertex shader

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) tex_coords: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
};

struct Uniforms {
    transform: mat4x4<f32>,
    tile_offset: vec2<f32>,
    vmin: f32,
    vmax: f32,
    tile_size: f32,
    _padding1: f32,
    _padding2: vec2<f32>,
    _padding3: vec4<f32>,
};

@group(1) @binding(0)
var<uniform> uniforms: Uniforms;

@vertex
fn vs_main(
    model: VertexInput,
) -> VertexOutput {
    var out: VertexOutput;
    out.tex_coords = model.tex_coords;
    
    // Scale the unit quad [0,1] to effective tile size and offset by tile position
    let tile_pos = model.position * uniforms.tile_size + uniforms.tile_offset;
    
    // Apply viewport transformation (pan, zoom, rotation) using the transformation matrix
    let transformed_2d = (uniforms.transform * vec4<f32>(tile_pos, 0.0, 1.0)).xy;
    
    // Render tiles at standard depth
    out.clip_position = vec4<f32>(transformed_2d, 0.0, 1.0);
    
    return out;
}

// Fragment shader

@group(0) @binding(0)
var t_diffuse: texture_2d<f32>;
@group(0) @binding(1)
var s_diffuse: sampler;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let sample = textureSampleLevel(t_diffuse, s_diffuse, in.tex_coords, 0.0);
    // For R32Float textures, the value is in the red channel
    let raw_value = sample.r;
    
    // Normalize the value from [vmin, vmax] to [0, 1]
    let normalized = (raw_value - uniforms.vmin) / (uniforms.vmax - uniforms.vmin);
    let intensity = clamp(normalized, 0.0, 1.0);
    
    return vec4<f32>(intensity, intensity, intensity, 1.0);
}
