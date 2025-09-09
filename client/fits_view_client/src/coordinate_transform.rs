use serde::{Deserialize, Serialize};
use crate::tile_manager::TileCoord;

/// Coordinate transformation functions following the pipeline specification
/// from fits_viewer_pipeline.md

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Viewport {
    pub zoom: f32,
    pub pan_offset: [f32; 2], // Use array for JSON compatibility
    pub rotation_angle: f32,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            pan_offset: [0.0, 0.0],
            rotation_angle: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RenderSize {
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ImageSize {
    pub width: f32,
    pub height: f32,
}

// TileCoord is now defined in tile_manager.rs

/// Calculate LOD based on zoom level following pipeline spec section 4.2
/// z_ideal = -log2(zoom)
/// lod_to_request = round(z_ideal)
pub fn calculate_lod(zoom: f32, max_lod: u32) -> u32 {
    if zoom <= 0.0 {
        return max_lod;
    }
    
    let z_ideal = -zoom.log2();
    let lod_to_request = z_ideal.round() as i32;
    
    // Clamp to valid range [0, max_lod]
    (lod_to_request.max(0) as u32).min(max_lod)
}

/// Calculate maximum useful LOD level based on image and tile size
pub fn calculate_max_lod(image_size: ImageSize, tile_size: u32) -> u32 {
    let min_image_dimension = image_size.width.min(image_size.height);
    
    if min_image_dimension <= tile_size as f32 {
        return 0;
    }
    
    let max_lod_float = (min_image_dimension / tile_size as f32).log2();
    (max_lod_float.floor() as u32).max(0)
}

/// Transform screen coordinates to image coordinates
/// Following pipeline spec section 2.3 inverse transformation
pub fn screen_to_image(
    screen_x: f32,
    screen_y: f32,
    viewport: &Viewport,
    render_size: RenderSize,
    image_size: ImageSize,
) -> (f32, f32) {
    let render_center_x = render_size.width / 2.0;
    let render_center_y = render_size.height / 2.0;
    let image_center_x = image_size.width / 2.0;
    let image_center_y = image_size.height / 2.0;
    
    // Inverse zoom and pan calculations
    let zoom_inv = 1.0 / viewport.zoom;
    let pan_in_image_x = -viewport.pan_offset[0] * zoom_inv;
    let pan_in_image_y = viewport.pan_offset[1] * zoom_inv;
    
    let center_x = image_center_x + pan_in_image_x;
    let center_y = image_center_y + pan_in_image_y;
    
    // Transform screen to image coordinates
    let ix = (screen_x - render_center_x) * zoom_inv + center_x;
    let iy = (screen_y - render_center_y) * zoom_inv + center_y;
    
    (ix, iy)
}

/// Calculate visible tiles following pipeline spec section 3
pub fn calculate_visible_tiles(
    viewport: &Viewport,
    render_size: RenderSize,
    image_size: ImageSize,
    tile_size: u32,
    max_lod: u32,
) -> Vec<TileCoord> {
    let mut visible_tiles = Vec::new();
    
    // Calculate LOD for current zoom level
    let lod = calculate_lod(viewport.zoom, max_lod);
    
    // Step 1: Define padded screen area corners (half tile size padding)
    let padding = tile_size as f32 * 0.5;
    let corners_screen = [
        (-padding, -padding),
        (render_size.width + padding, -padding),
        (render_size.width + padding, render_size.height + padding),
        (-padding, render_size.height + padding),
    ];
    
    // Step 2: Transform corners to image space
    let mut corners_image = Vec::new();
    for (sx, sy) in corners_screen.iter() {
        let (ix, iy) = screen_to_image(*sx, *sy, viewport, render_size, image_size);
        corners_image.push((ix, iy));
    }
    
    // Step 3: Calculate AABB in image space
    let min_x = corners_image.iter().map(|(x, _)| *x).fold(f32::INFINITY, f32::min);
    let max_x = corners_image.iter().map(|(x, _)| *x).fold(f32::NEG_INFINITY, f32::max);
    let min_y = corners_image.iter().map(|(_, y)| *y).fold(f32::INFINITY, f32::min);
    let max_y = corners_image.iter().map(|(_, y)| *y).fold(f32::NEG_INFINITY, f32::max);
    
    // Step 4: Convert to tile indices accounting for LOD
    let lod_scale = 2_u32.pow(lod) as f32;
    let effective_tile_size = tile_size as f32 * lod_scale;
    
    let min_tx = (min_x / effective_tile_size).floor() as i32;
    let max_tx = (max_x / effective_tile_size).ceil() as i32;
    let min_ty = (min_y / effective_tile_size).floor() as i32;
    let max_ty = (max_y / effective_tile_size).ceil() as i32;
    
    // Generate list of visible tile coordinates
    for ty in min_ty..max_ty {
        for tx in min_tx..max_tx {
            // Ensure tiles are within image bounds
            if is_tile_in_bounds(tx, ty, lod, image_size, tile_size) {
                visible_tiles.push(TileCoord {
                    x: tx as u32,
                    y: ty as u32,
                    lod,
                });
            }
        }
    }
    
    visible_tiles
}

/// Check if a tile coordinate is within image bounds for the given LOD
fn is_tile_in_bounds(tx: i32, ty: i32, lod: u32, image_size: ImageSize, tile_size: u32) -> bool {
    if tx < 0 || ty < 0 {
        return false;
    }
    
    let lod_scale = 2_u32.pow(lod) as f32;
    let effective_image_width = image_size.width / lod_scale;
    let effective_image_height = image_size.height / lod_scale;
    let effective_tile_size = tile_size as f32;
    
    let max_tiles_x = (effective_image_width / effective_tile_size).ceil() as u32;
    let max_tiles_y = (effective_image_height / effective_tile_size).ceil() as u32;
    
    (tx as u32) < max_tiles_x && (ty as u32) < max_tiles_y
}

/// Calculate image screen extent following pipeline spec
pub fn calculate_image_screen_extent(
    viewport: &Viewport,
    render_size: RenderSize,
    image_size: ImageSize,
) -> ImageScreenExtent {
    let render_center_x = render_size.width / 2.0;
    let render_center_y = render_size.height / 2.0;
    let image_center_x = image_size.width / 2.0;
    let image_center_y = image_size.height / 2.0;
    
    let zoom_inv = 1.0 / viewport.zoom;
    let pan_in_image_x = -viewport.pan_offset[0] * zoom_inv;
    let pan_in_image_y = viewport.pan_offset[1] * zoom_inv;
    
    let center_x = image_center_x + pan_in_image_x;
    let center_y = image_center_y + pan_in_image_y;
    
    // Transform image corners to screen coordinates
    let image_min_x = ((0.0 - center_x) * viewport.zoom + render_center_x).round() as i32;
    let image_min_y = ((0.0 - center_y) * viewport.zoom + render_center_y).round() as i32;
    let image_max_x = ((image_size.width - center_x) * viewport.zoom + render_center_x).round() as i32;
    let image_max_y = ((image_size.height - center_y) * viewport.zoom + render_center_y).round() as i32;
    
    ImageScreenExtent {
        min: [image_min_x, image_min_y],
        max: [image_max_x, image_max_y],
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageScreenExtent {
    pub min: [i32; 2],
    pub max: [i32; 2],
}

/// Generate visibility mask string representation for testing
pub fn generate_visibility_mask(
    visible_tiles: &[TileCoord],
    lod: u32,
    image_size: ImageSize,
    tile_size: u32,
) -> Vec<String> {
    let lod_scale = 2_u32.pow(lod) as f32;
    let effective_image_width = image_size.width / lod_scale;
    let effective_image_height = image_size.height / lod_scale;
    let effective_tile_size = tile_size as f32;
    
    let max_tiles_x = (effective_image_width / effective_tile_size).ceil() as u32;
    let max_tiles_y = (effective_image_height / effective_tile_size).ceil() as u32;
    
    // Create set of visible tile coordinates for fast lookup
    let visible_set: std::collections::HashSet<(u32, u32)> = visible_tiles
        .iter()
        .map(|tile| (tile.x, tile.y))
        .collect();
    
    // Generate mask rows (top to bottom)
    let mut mask_rows = Vec::new();
    for ty in (0..max_tiles_y).rev() {  // Top to bottom
        let mut row = String::new();
        for tx in 0..max_tiles_x {
            if visible_set.contains(&(tx, ty)) {
                row.push('1');
            } else {
                row.push('0');
            }
        }
        mask_rows.push(row);
    }
    
    mask_rows
}

/// Create M_image_to_ndc matrix - simplified version that properly centers the image
pub fn create_image_to_ndc_matrix(
    viewport: &Viewport,
    render_size: RenderSize,
    image_size: ImageSize,
) -> [[f32; 4]; 4] {
    // Calculate scale to fit image in viewport with zoom
    let scale_x = (2.0 * viewport.zoom) / render_size.width;
    let scale_y = (2.0 * viewport.zoom) / render_size.height;
    
    // Calculate translation to center the image at NDC origin (0,0)
    // We need to translate by -image_center in image space, then apply scale
    let image_center_x = image_size.width * 0.5;
    let image_center_y = image_size.height * 0.5;
    
    // Translate to center the image at NDC origin (0,0), then apply pan offset
    let translate_x = -image_center_x * scale_x + viewport.pan_offset[0];
    let translate_y = -image_center_y * scale_y + viewport.pan_offset[1];
    
    // Matrix in column-major format for WGSL
    [
        [scale_x, 0.0, 0.0, 0.0],
        [0.0, scale_y, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [translate_x, translate_y, 0.0, 1.0],
    ]
}

/// Matrix multiplication helper (column-major)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lod_calculation() {
        assert_eq!(calculate_lod(1.0, 10), 0);  // zoom=1.0 -> LOD 0
        assert_eq!(calculate_lod(0.5, 10), 1);  // zoom=0.5 -> LOD 1
        assert_eq!(calculate_lod(0.25, 10), 2); // zoom=0.25 -> LOD 2
        assert_eq!(calculate_lod(2.0, 10), 0);  // zoom=2.0 -> LOD 0 (clamped)
    }

    #[test]
    fn test_max_lod_calculation() {
        let image_size = ImageSize { width: 2048.0, height: 2048.0 };
        let max_lod = calculate_max_lod(image_size, 256);
        assert_eq!(max_lod, 3); // floor(log2(2048/256)) = floor(3.0) = 3
        
        let small_image = ImageSize { width: 200.0, height: 200.0 };
        let max_lod_small = calculate_max_lod(small_image, 256);
        assert_eq!(max_lod_small, 0); // Image smaller than tile
    }

    #[test]
    fn test_screen_to_image_identity() {
        let viewport = Viewport::default();
        let render_size = RenderSize { width: 800.0, height: 600.0 };
        let image_size = ImageSize { width: 800.0, height: 600.0 };
        
        // Center of screen should map to center of image
        let (ix, iy) = screen_to_image(400.0, 300.0, &viewport, render_size, image_size);
        assert!((ix - 400.0).abs() < 0.001);
        assert!((iy - 300.0).abs() < 0.001);
    }
}
