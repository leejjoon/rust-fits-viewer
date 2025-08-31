use fits_view_client::{MetaResponse, Viewport};
use std::collections::HashMap;

/// Simple test for tile rendering with a 2x2 tile grid (512x512 image)
/// Each tile will have a distinct solid color for easy visual verification
pub struct SimpleTileServer {
    pub meta: MetaResponse,
    pub tiles: HashMap<(u32, u32, u32), Vec<u8>>, // (z, x, y) -> raw data
}

impl SimpleTileServer {
    pub fn new() -> Self {
        // Create a simple 2x2 tile grid (512x512 image with 256x256 tiles)
        let meta = MetaResponse {
            shape: [512, 512],
            tile_size: 256,
            min_val: 0.0,
            max_val: 255.0,
        };
        
        let mut tiles = HashMap::new();
        
        // Generate 4 tiles with distinct solid colors
        let colors = [
            100.0, // Top-left: dark gray
            200.0, // Top-right: light gray  
            50.0,  // Bottom-left: very dark
            255.0, // Bottom-right: white
        ];
        
        for y in 0..2u32 {
            for x in 0..2u32 {
                let color_index = (y * 2 + x) as usize;
                let tile_data = generate_solid_tile(colors[color_index]);
                tiles.insert((0, x, y), tile_data);
                println!("📦 Generated tile ({}, {}) with color {}", x, y, colors[color_index]);
            }
        }
        
        Self { meta, tiles }
    }
    
    pub fn get_tile_data(&self, z: u32, x: u32, y: u32) -> Option<&Vec<u8>> {
        self.tiles.get(&(z, x, y))
    }
    
    pub fn verify_tile_pattern(&self) {
        println!("🎨 Expected tile pattern (2x2 grid):");
        println!("   ┌─────────┬─────────┐");
        println!("   │ (0,0)   │ (1,0)   │");
        println!("   │ 100.0   │ 200.0   │");
        println!("   ├─────────┼─────────┤");
        println!("   │ (0,1)   │ (1,1)   │");
        println!("   │ 50.0    │ 255.0   │");
        println!("   └─────────┴─────────┘");
    }
}

/// Generate a solid color tile (256x256 pixels)
fn generate_solid_tile(color_value: f32) -> Vec<u8> {
    let tile_size = 256;
    let mut data = Vec::with_capacity(tile_size * tile_size * 4); // 4 bytes per f32
    
    for _y in 0..tile_size {
        for _x in 0..tile_size {
            // All pixels have the same color value
            data.extend_from_slice(&color_value.to_le_bytes());
        }
    }
    
    data
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_simple_tile_server_creation() {
        let server = SimpleTileServer::new();
        
        // Verify metadata for 2x2 grid
        assert_eq!(server.meta.shape, [512, 512]);
        assert_eq!(server.meta.tile_size, 256);
        
        // Verify all 4 tiles are generated
        assert_eq!(server.tiles.len(), 4);
        
        // Verify each tile exists and has correct size
        for y in 0..2u32 {
            for x in 0..2u32 {
                let tile_data = server.get_tile_data(0, x, y).unwrap();
                assert_eq!(tile_data.len(), 256 * 256 * 4); // 256x256 f32 values
            }
        }
        
        server.verify_tile_pattern();
        println!("✅ Simple tile server test passed");
    }
    
    #[test]
    fn test_viewport_for_simple_image() {
        let server = SimpleTileServer::new();
        
        // Test viewport setup for 512x512 image in 800x600 window
        let mut viewport = Viewport::default();
        let window_size = egui::Vec2::new(800.0, 600.0);
        let image_size = egui::Vec2::new(512.0, 512.0);
        
        viewport.fit_to_window(window_size, image_size);
        
        // For 512x512 image in 800x600 window, should be limited by height
        let expected_zoom = 600.0_f32 / 512.0_f32; // ~1.17
        assert!((viewport.zoom - expected_zoom).abs() < 0.001);
        
        // Pan should be zero (centered)
        assert_eq!(viewport.pan_offset, egui::Vec2::ZERO);
        
        println!("✅ Viewport setup test passed - zoom: {:.3}", viewport.zoom);
    }
    
    #[test]
    fn test_tile_visibility_for_simple_image() {
        let server = SimpleTileServer::new();
        
        // Test tile visibility calculation
        let viewport = Viewport {
            zoom: 1.0,
            pan_offset: egui::Vec2::ZERO,
            rotation_angle: 0.0,
        };
        
        let render_size = egui::Vec2::new(800.0, 600.0);
        let image_size = egui::Vec2::new(512.0, 512.0);
        let visible_tiles = viewport.get_visible_tiles(render_size, image_size, 256);
        
        // At zoom 1.0 with no pan, should see all 4 tiles
        assert_eq!(visible_tiles.len(), 4);
        
        // Verify we have tiles (0,0), (1,0), (0,1), (1,1)
        let mut coords: Vec<(u32, u32)> = visible_tiles.iter().map(|t| (t.x, t.y)).collect();
        coords.sort();
        
        let expected_coords = vec![(0, 0), (0, 1), (1, 0), (1, 1)];
        assert_eq!(coords, expected_coords);
        
        println!("✅ Tile visibility test passed - {} tiles visible", visible_tiles.len());
        
        // Print tile coordinates for verification
        for tile in &visible_tiles {
            println!("   Visible tile: ({}, {})", tile.x, tile.y);
        }
    }
    
    #[test]
    fn test_tile_data_uniqueness() {
        let server = SimpleTileServer::new();
        
        // Verify each tile has different data (different colors)
        let tile_00 = server.get_tile_data(0, 0, 0).unwrap();
        let tile_10 = server.get_tile_data(0, 1, 0).unwrap();
        let tile_01 = server.get_tile_data(0, 0, 1).unwrap();
        let tile_11 = server.get_tile_data(0, 1, 1).unwrap();
        
        // All tiles should be different
        assert_ne!(tile_00, tile_10);
        assert_ne!(tile_00, tile_01);
        assert_ne!(tile_00, tile_11);
        assert_ne!(tile_10, tile_01);
        assert_ne!(tile_10, tile_11);
        assert_ne!(tile_01, tile_11);
        
        // Verify the actual color values by checking first pixel of each tile
        let get_first_pixel = |data: &[u8]| -> f32 {
            f32::from_le_bytes([data[0], data[1], data[2], data[3]])
        };
        
        assert_eq!(get_first_pixel(tile_00), 100.0); // Top-left
        assert_eq!(get_first_pixel(tile_10), 200.0); // Top-right
        assert_eq!(get_first_pixel(tile_01), 50.0);  // Bottom-left
        assert_eq!(get_first_pixel(tile_11), 255.0); // Bottom-right
        
        println!("✅ Tile data uniqueness test passed");
        println!("   Tile (0,0): {}", get_first_pixel(tile_00));
        println!("   Tile (1,0): {}", get_first_pixel(tile_10));
        println!("   Tile (0,1): {}", get_first_pixel(tile_01));
        println!("   Tile (1,1): {}", get_first_pixel(tile_11));
    }
}
