// Re-export types for testing
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MetaResponse {
    pub shape: [u32; 2],
    pub tile_size: u32,
    pub min_val: f32,
    pub max_val: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct Viewport {
    pub zoom: f32,
    pub pan_offset: egui::Vec2,
    pub rotation_angle: f32,
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
    pub fn fit_to_window(&mut self, window_size: egui::Vec2, image_size: egui::Vec2) {
        let scale_x = window_size.x / image_size.x;
        let scale_y = window_size.y / image_size.y;
        self.zoom = scale_x.min(scale_y);
        self.pan_offset = egui::Vec2::ZERO;
        self.rotation_angle = 0.0;
    }
    
    pub fn get_visible_tiles(&self, render_size: egui::Vec2, image_size: egui::Vec2, tile_size: u32) -> Vec<TileCoord> {
        let mut visible_tiles = Vec::new();
        
        // Calculate the bounds of the visible area in image coordinates
        let half_render = render_size * 0.5;
        let image_center = image_size * 0.5;
        
        // Transform viewport bounds to image coordinates
        let zoom_inv = 1.0 / self.zoom;
        let visible_half_width = half_render.x * zoom_inv;
        let visible_half_height = half_render.y * zoom_inv;
        
        // Add padding to include edge tiles
        let padding_factor = 1.5;
        let expanded_half_width = visible_half_width * padding_factor;
        let expanded_half_height = visible_half_height * padding_factor;
        
        // Apply pan offset to determine which part of the image is visible
        // Pan offset should be applied directly in image coordinates
        let pan_in_image_x = -self.pan_offset.x * zoom_inv;
        let pan_in_image_y = self.pan_offset.y * zoom_inv;
        
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
        
        for y in min_tile_y..max_tile_y {
            for x in min_tile_x..max_tile_x {
                if x < max_tiles_x && y < max_tiles_y {
                    visible_tiles.push(TileCoord { z: 0, x, y });
                }
            }
        }
        
        visible_tiles
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TileCoord {
    pub z: u32,
    pub x: u32,
    pub y: u32,
}
