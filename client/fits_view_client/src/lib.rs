// Re-export types for testing
pub mod coordinate_transform;
pub mod test_data_validation;
pub mod tile_manager;

use serde::{Deserialize, Serialize};
pub use coordinate_transform::*;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MetaResponse {
    pub shape: [u32; 2],
    pub tile_size: u32,
    pub min_val: f32,
    pub max_val: f32,
}

