use std::collections::HashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, Hash, PartialEq, Eq)]
pub struct TileCoord {
    pub lod: u32,
    pub x: u32,
    pub y: u32,
}

pub struct TileManager {
    pub tiles: HashMap<(u32, u32, u32), Vec<f32>>,
    pub tile_size: u32,
    pub image_size: [u32; 2],
}

impl TileManager {
    pub fn new(tile_size: u32, image_size: [u32; 2]) -> Self {
        Self {
            tiles: HashMap::new(),
            tile_size,
            image_size,
        }
    }

    pub fn get_or_create_tile(&mut self, coord: &TileCoord) -> Vec<f32> {
        let key = (coord.lod, coord.x, coord.y);
        
        if let Some(tile_data) = self.tiles.get(&key) {
            return tile_data.clone();
        }

        // Generate checkerboard test pattern
        let tile_size = self.tile_size as usize;
        let mut tile_data = vec![0.0; tile_size * tile_size];
        
        for y in 0..tile_size {
            for x in 0..tile_size {
                let checker_x = (x / 32) % 2;
                let checker_y = (y / 32) % 2;
                let value = if checker_x == checker_y { 1.0 } else { 0.0 };
                tile_data[y * tile_size + x] = value;
            }
        }
        
        self.tiles.insert(key, tile_data.clone());
        tile_data
    }
}
