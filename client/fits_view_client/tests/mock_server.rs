use std::collections::HashMap;

/// Simple mock server for testing with predictable tile data
pub struct MockTileServer {
    pub meta: fits_view_client::MetaResponse,
    pub tiles: HashMap<(u32, u32, u32), Vec<u8>>, // (z, x, y) -> raw data
}

impl MockTileServer {
    pub fn new() -> Self {
        let meta = fits_view_client::MetaResponse {
            shape: [1024, 1024],
            tile_size: 256,
            min_val: 0.0,
            max_val: 255.0,
        };
        
        let mut tiles = HashMap::new();
        
        // Generate predictable test tiles (4x4 grid)
        for y in 0..4 {
            for x in 0..4 {
                let tile_data = generate_test_tile(x, y);
                tiles.insert((0, x, y), tile_data);
            }
        }
        
        Self { meta, tiles }
    }
    
    pub fn get_meta_json(&self) -> String {
        serde_json::to_string(&self.meta).unwrap()
    }
    
    pub fn get_tile_data(&self, z: u32, x: u32, y: u32) -> Option<&Vec<u8>> {
        self.tiles.get(&(z, x, y))
    }
}

/// Generate a test tile with a predictable pattern
/// Each tile will have a unique pattern based on its coordinates
fn generate_test_tile(tile_x: u32, tile_y: u32) -> Vec<u8> {
    let tile_size = 256;
    let mut data = Vec::with_capacity(tile_size * tile_size * 4); // 4 bytes per f32
    
    for y in 0..tile_size {
        for x in 0..tile_size {
            // Create a pattern that's unique per tile and has visual structure
            let value = if ((x / 32) + (y / 32) + (tile_x as usize) + (tile_y as usize)) % 2 == 0 {
                // Bright squares
                200.0 + (tile_x * 10) as f32 + (tile_y * 5) as f32
            } else {
                // Dark squares  
                50.0 + (tile_x * 5) as f32 + (tile_y * 2) as f32
            };
            
            // Add some gradient within each square
            let gradient = (x % 32) as f32 * 0.5 + (y % 32) as f32 * 0.3;
            let final_value = (value + gradient).clamp(0.0, 255.0);
            
            // Convert to little-endian bytes
            data.extend_from_slice(&final_value.to_le_bytes());
        }
    }
    
    data
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_mock_server_creation() {
        let server = MockTileServer::new();
        
        // Verify metadata
        assert_eq!(server.meta.shape, [1024, 1024]);
        assert_eq!(server.meta.tile_size, 256);
        
        // Verify all 16 tiles are generated
        assert_eq!(server.tiles.len(), 16);
        
        // Verify tile data size
        let tile_data = server.get_tile_data(0, 0, 0).unwrap();
        assert_eq!(tile_data.len(), 256 * 256 * 4); // 256x256 f32 values
    }
    
    #[test]
    fn test_tile_patterns_are_unique() {
        let server = MockTileServer::new();
        
        let tile_00 = server.get_tile_data(0, 0, 0).unwrap();
        let tile_01 = server.get_tile_data(0, 0, 1).unwrap();
        let tile_10 = server.get_tile_data(0, 1, 0).unwrap();
        
        // Tiles should have different patterns
        assert_ne!(tile_00, tile_01);
        assert_ne!(tile_00, tile_10);
        assert_ne!(tile_01, tile_10);
    }
}
