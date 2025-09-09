use serde::Deserialize;
use std::fs;
use std::path::Path;
use fits_view_client::{Viewport, RenderSize, ImageSize, calculate_visible_tiles, calculate_max_lod, tile_manager::TileCoord, generate_visibility_mask};

#[derive(Deserialize, Debug)]
struct TestCase {
    name: String,
    input: TestInput,
    output: TestOutput,
}

#[derive(Deserialize, Debug)]
struct TestInput {
    render_size: [f32; 2],
    image_size: [f32; 2],
    zoom: f32,
    center_on_image: [f32; 2],
    tile_size: u32,
    #[serde(default)]
    lod: u32,
}

#[derive(Deserialize, Debug)]
struct TestOutput {
    visibility_mask: Vec<String>,
    #[serde(default)]
    lod: u32,
}

#[test]
fn test_visible_tile_calculation() {
    let test_cases_dir = Path::new(file!())
        .parent()
        .unwrap()
        .join("test_cases_generated");
    
    let mut test_cases = Vec::new();
    
    for entry in fs::read_dir(&test_cases_dir).expect("Failed to read test_cases directory") {
        let entry = entry.expect("Failed to read directory entry");
        let path = entry.path();
        
        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            let json_str = fs::read_to_string(&path)
                .expect(&format!("Failed to read test case file: {:?}", path));
            let test_case: TestCase = serde_json::from_str(&json_str)
                .expect(&format!("Failed to parse JSON from file: {:?}", path));
            test_cases.push(test_case);
        }
    }
    
    test_cases.sort_by(|a, b| a.name.cmp(&b.name));

    for case in test_cases {
        println!("Testing case: {}", case.name);

        let viewport = Viewport {
            zoom: case.input.zoom,
            center_on_image: case.input.center_on_image,
            rotation_angle: 0.0,
        };

        let render_size = RenderSize {
            width: case.input.render_size[0],
            height: case.input.render_size[1],
        };
        
        let image_size = ImageSize {
            width: case.input.image_size[0],
            height: case.input.image_size[1],
        };
        
        let max_lod = calculate_max_lod(image_size, case.input.tile_size);
        let visible_tiles = calculate_visible_tiles(&viewport, render_size, image_size, case.input.tile_size, max_lod);
        let calculated_mask = generate_visibility_mask(&visible_tiles, case.output.lod, image_size, case.input.tile_size);

        assert_eq!(calculated_mask, case.output.visibility_mask, "Mismatch in case: {}", case.name);
    }
}
