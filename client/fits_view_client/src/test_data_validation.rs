use crate::coordinate_transform::*;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize, Serialize)]
pub struct TestCaseInput {
    pub render_size: [f32; 2],
    pub image_size: [f32; 2],
    pub zoom: f32,
    pub pan_offset: [f32; 2],
    pub tile_size: u32,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TestCaseOutput {
    #[serde(default)]
    pub lod: u32,
    pub padded_area: [f32; 4],
    pub visibility_mask: Vec<String>,
    pub image_screen_extent: ImageScreenExtent,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TestCase {
    pub name: String,
    pub input: TestCaseInput,
    pub output: TestCaseOutput,
}

/// Load test case from JSON file
pub fn load_test_case<P: AsRef<Path>>(path: P) -> Result<TestCase, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(path)?;
    let test_case: TestCase = serde_json::from_str(&content)?;
    Ok(test_case)
}

/// Validate LOD calculation against test data
pub fn validate_lod_calculation(test_case: &TestCase) -> Result<(), String> {
    let _render_size = RenderSize {
        width: test_case.input.render_size[0],
        height: test_case.input.render_size[1],
    };
    let image_size = ImageSize {
        width: test_case.input.image_size[0],
        height: test_case.input.image_size[1],
    };
    
    let max_lod = calculate_max_lod(image_size, test_case.input.tile_size);
    let calculated_lod = calculate_lod(test_case.input.zoom, max_lod);
    let expected_lod = test_case.output.lod;
    
    if calculated_lod != expected_lod {
        return Err(format!(
            "LOD mismatch: calculated={}, expected={}",
            calculated_lod, expected_lod
        ));
    }
    
    Ok(())
}

/// Validate image screen extent calculation against test data
pub fn validate_image_screen_extent(test_case: &TestCase) -> Result<(), String> {
    let viewport = Viewport {
        zoom: test_case.input.zoom,
        pan_offset: test_case.input.pan_offset,
        rotation_angle: 0.0, // Test data doesn't include rotation yet
    };
    
    let render_size = RenderSize {
        width: test_case.input.render_size[0],
        height: test_case.input.render_size[1],
    };
    let image_size = ImageSize {
        width: test_case.input.image_size[0],
        height: test_case.input.image_size[1],
    };
    
    let calculated_extent = calculate_image_screen_extent(&viewport, render_size, image_size);
    let expected_extent = &test_case.output.image_screen_extent;
    
    if calculated_extent.min != expected_extent.min || calculated_extent.max != expected_extent.max {
        return Err(format!(
            "Image screen extent mismatch:\ncalculated: min={:?}, max={:?}\nexpected: min={:?}, max={:?}",
            calculated_extent.min, calculated_extent.max,
            expected_extent.min, expected_extent.max
        ));
    }
    
    Ok(())
}

/// Validate tile selection against test data
pub fn validate_tile_selection(test_case: &TestCase) -> Result<(), String> {
    let viewport = Viewport {
        zoom: test_case.input.zoom,
        pan_offset: test_case.input.pan_offset,
        rotation_angle: 0.0,
    };
    
    let render_size = RenderSize {
        width: test_case.input.render_size[0],
        height: test_case.input.render_size[1],
    };
    let image_size = ImageSize {
        width: test_case.input.image_size[0],
        height: test_case.input.image_size[1],
    };
    
    let max_lod = calculate_max_lod(image_size, test_case.input.tile_size);
    let visible_tiles = calculate_visible_tiles(&viewport, render_size, image_size, test_case.input.tile_size, max_lod);
    
    // Generate visibility mask from calculated tiles
    let calculated_mask = generate_visibility_mask(&visible_tiles, test_case.output.lod, image_size, test_case.input.tile_size);
    let expected_mask = &test_case.output.visibility_mask;
    
    if calculated_mask != *expected_mask {
        return Err(format!(
            "Visibility mask mismatch:\ncalculated:\n{}\nexpected:\n{}",
            calculated_mask.join("\n"),
            expected_mask.join("\n")
        ));
    }
    
    Ok(())
}

/// Validate all aspects of a test case
pub fn validate_test_case(test_case: &TestCase) -> Result<(), String> {
    println!("Validating test case: {}", test_case.name);
    
    // Validate LOD calculation
    if let Err(e) = validate_lod_calculation(test_case) {
        return Err(format!("LOD validation failed: {}", e));
    }
    
    // Validate image screen extent
    if let Err(e) = validate_image_screen_extent(test_case) {
        return Err(format!("Image screen extent validation failed: {}", e));
    }
    
    // Validate tile selection
    if let Err(e) = validate_tile_selection(test_case) {
        return Err(format!("Tile selection validation failed: {}", e));
    }
    
    println!("✅ Test case '{}' passed all validations", test_case.name);
    Ok(())
}

/// Run validation on all test cases in a directory
pub fn validate_all_test_cases<P: AsRef<Path>>(test_dir: P) -> Result<(), Box<dyn std::error::Error>> {
    let test_dir = test_dir.as_ref();
    let mut test_files = Vec::new();
    
    // Collect all JSON files
    for entry in fs::read_dir(test_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            test_files.push(path);
        }
    }
    
    test_files.sort();
    
    let mut passed = 0;
    let mut failed = 0;
    
    for test_file in test_files {
        match load_test_case(&test_file) {
            Ok(test_case) => {
                match validate_test_case(&test_case) {
                    Ok(()) => passed += 1,
                    Err(e) => {
                        eprintln!("❌ Test case '{}' failed: {}", test_case.name, e);
                        failed += 1;
                    }
                }
            }
            Err(e) => {
                eprintln!("❌ Failed to load test case {:?}: {}", test_file, e);
                failed += 1;
            }
        }
    }
    
    println!("\n📊 Test Results: {} passed, {} failed", passed, failed);
    
    if failed > 0 {
        Err("Some test cases failed".into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_load_test_case() {
        let test_data = r#"{
            "name": "Test case",
            "input": {
                "render_size": [800.0, 600.0],
                "image_size": [1024.0, 1024.0],
                "zoom": 1.0,
                "pan_offset": [0.0, 0.0],
                "tile_size": 256
            },
            "output": {
                "lod": 0,
                "padded_area": [-128.0, -128.0, 1056.0, 856.0],
                "visibility_mask": ["1111", "1111", "1111", "1111"],
                "image_screen_extent": {
                    "min": [-112, -212],
                    "max": [912, 812]
                }
            }
        }"#;
        
        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(test_data.as_bytes()).unwrap();
        
        let test_case = load_test_case(temp_file.path()).unwrap();
        assert_eq!(test_case.name, "Test case");
        assert_eq!(test_case.input.zoom, 1.0);
        assert_eq!(test_case.output.lod, 0);
    }
}
