use serde::{Deserialize, Serialize};
use crate::tile_manager::TileCoord;

/// Coordinate transformation functions following the pipeline specification
/// from fits_viewer_pipeline.md

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Viewport {
    pub zoom: f32,
    pub center_on_image: [f32; 2], // The image point at the center of the viewport
    pub rotation_angle: f32,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            center_on_image: [0.0, 0.0], // Caller should set a proper default (e.g., image center)
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

// ... (TileCoord is in tile_manager.rs)

/// Calculate LOD based on zoom level
pub fn calculate_lod(zoom: f32, max_lod: u32) -> u32 {
    if zoom <= 0.0 {
        return max_lod;
    }
    let z_ideal = -zoom.log2();
    let lod_to_request = z_ideal.round() as i32;
    (lod_to_request.max(0) as u32).min(max_lod)
}

/// Calculate maximum useful LOD level
/// Ensures at least 2 LOD levels (0 and 1) for progressive loading
pub fn calculate_max_lod(image_size: ImageSize, tile_size: u32) -> u32 {
    let min_dim = image_size.width.min(image_size.height);
    if min_dim <= tile_size as f32 {
        // For very small images, we'll still use LOD 0 and 1 for consistency
        return 1;
    }
    let max_lod_float = (min_dim / tile_size as f32).log2();
    // Ensure we have at least 2 LOD levels
    (max_lod_float.floor() as u32).max(1)
}

/// Transform screen coordinates to image coordinates
pub fn screen_to_image(
    screen_x: f32,
    screen_y: f32,
    viewport: &Viewport,
    render_size: RenderSize,
) -> (f32, f32) {
    let ndc_x = (screen_x / render_size.width) * 2.0 - 1.0;
    let ndc_y = (screen_y / render_size.height) * 2.0 - 1.0;

    let scale_x = (2.0 * viewport.zoom) / render_size.width;
    let scale_y = (2.0 * viewport.zoom) / render_size.height;

    let ix = (ndc_x / scale_x) + viewport.center_on_image[0];
    let iy = (ndc_y / scale_y) + viewport.center_on_image[1];

    (ix, iy)
}


/// Calculate visible tiles
pub fn calculate_visible_tiles(
    viewport: &Viewport,
    render_size: RenderSize,
    image_size: ImageSize,
    tile_size: u32,
    lod: u32,  // Changed from max_lod to lod since we want to use the specific LOD level
) -> Vec<TileCoord> {
    // Use the provided lod parameter directly instead of recalculating
    let padding = tile_size as f32 * 0.5;
    let corners_screen = [
        (-padding, -padding),
        (render_size.width + padding, -padding),
        (render_size.width + padding, render_size.height + padding),
        (-padding, render_size.height + padding),
    ];

    let corners_image: Vec<_> = corners_screen
        .iter()
        .map(|(sx, sy)| screen_to_image(*sx, *sy, viewport, render_size))
        .collect();

    let min_x = corners_image.iter().map(|(x, _)| *x).fold(f32::INFINITY, f32::min);
    let max_x = corners_image.iter().map(|(x, _)| *x).fold(f32::NEG_INFINITY, f32::max);
    let min_y = corners_image.iter().map(|(_, y)| *y).fold(f32::INFINITY, f32::min);
    let max_y = corners_image.iter().map(|(_, y)| *y).fold(f32::NEG_INFINITY, f32::max);

    let lod_scale = 2_u32.pow(lod) as f32;
    let effective_tile_size = tile_size as f32 * lod_scale;

    let min_tx = (min_x / effective_tile_size).floor() as i32;
    let max_tx = (max_x / effective_tile_size).ceil() as i32;
    let min_ty = (min_y / effective_tile_size).floor() as i32;
    let max_ty = (max_y / effective_tile_size).ceil() as i32;

    let mut visible_tiles = Vec::new();
    for ty in min_ty..max_ty {
        for tx in min_tx..max_tx {
            if is_tile_in_bounds(tx, ty, lod, image_size, tile_size) {
                visible_tiles.push(TileCoord { x: tx as u32, y: ty as u32, lod });
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
    let max_tiles_x = (image_size.width / lod_scale / tile_size as f32).ceil() as u32;
    let max_tiles_y = (image_size.height / lod_scale / tile_size as f32).ceil() as u32;
    (tx as u32) < max_tiles_x && (ty as u32) < max_tiles_y
}

/// Create M_image_to_ndc matrix
pub fn create_image_to_ndc_matrix(
    viewport: &Viewport,
    render_size: RenderSize,
) -> [[f32; 4]; 4] {
    let scale_x = (2.0 * viewport.zoom) / render_size.width;
    let scale_y = (2.0 * viewport.zoom) / render_size.height;

    // Translate by -center_on_image and then scale
    let translate_x = -viewport.center_on_image[0] * scale_x;
    let translate_y = -viewport.center_on_image[1] * scale_y;

    // Column-major matrix for WGSL
    [
        [scale_x, 0.0, 0.0, 0.0],
        [0.0, scale_y, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [translate_x, translate_y, 0.0, 1.0],
    ]
}

// Functions below are for testing and debugging, might need updates

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
    let max_tiles_x = (image_size.width / lod_scale / tile_size as f32).ceil() as u32;
    let max_tiles_y = (image_size.height / lod_scale / tile_size as f32).ceil() as u32;

    let visible_set: std::collections::HashSet<(u32, u32)> = visible_tiles.iter().map(|t| (t.x, t.y)).collect();

    let mut mask_rows = Vec::new();
    for ty in (0..max_tiles_y).rev() {
        let row: String = (0..max_tiles_x)
            .map(|tx| if visible_set.contains(&(tx, ty)) { '1' } else { '0' })
            .collect();
        mask_rows.push(row);
    }
    mask_rows
}

/// Calculate image screen extent
pub fn calculate_image_screen_extent(
    viewport: &Viewport,
    render_size: RenderSize,
    image_size: ImageSize,
) -> ImageScreenExtent {
    let scale_x = (2.0 * viewport.zoom) / render_size.width;
    let scale_y = (2.0 * viewport.zoom) / render_size.height;

    let image_to_screen = |ix: f32, iy: f32| {
        let ndc_x = (ix - viewport.center_on_image[0]) * scale_x;
        let ndc_y = (iy - viewport.center_on_image[1]) * scale_y;
        let sx = (ndc_x + 1.0) * render_size.width / 2.0;
        let sy = (ndc_y + 1.0) * render_size.height / 2.0;
        (sx.round() as i32, sy.round() as i32)
    };

    let (min_sx, min_sy) = image_to_screen(0.0, 0.0);
    let (max_sx, max_sy) = image_to_screen(image_size.width, image_size.height);

    ImageScreenExtent {
        min: [min_sx, min_sy],
        max: [max_sx, max_sy],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lod_calculation() {
        assert_eq!(calculate_lod(1.0, 10), 0);
        assert_eq!(calculate_lod(0.5, 10), 1);
        assert_eq!(calculate_lod(0.25, 10), 2);
        assert_eq!(calculate_lod(2.0, 10), 0);
    }

    #[test]
    fn test_max_lod_calculation() {
        let image_size = ImageSize { width: 2048.0, height: 2048.0 };
        assert_eq!(calculate_max_lod(image_size, 256), 3);
        let small_image = ImageSize { width: 200.0, height: 200.0 };
        assert_eq!(calculate_max_lod(small_image, 256), 0);
    }

    #[test]
    fn test_screen_to_image_identity() {
        let mut viewport = Viewport::default();
        let render_size = RenderSize { width: 800.0, height: 600.0 };
        viewport.center_on_image = [400.0, 300.0]; // Center of image

        let (ix, iy) = screen_to_image(400.0, 300.0, &viewport, render_size);
        assert!((ix - 400.0).abs() < 1e-3);
        assert!((iy - 300.0).abs() < 1e-3);
    }
}