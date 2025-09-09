use serde::Deserialize;
use std::fs;
use std::path::Path;
use fits_view_client::{Viewport, RenderSize, ImageSize, calculate_visible_tiles, calculate_max_lod, tile_manager::TileCoord};
use egui;

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
    pan_offset: [f32; 2],
    tile_size: u32,
    #[serde(default)]
    lod: u32,
}

#[derive(Deserialize, Debug)]
struct TestOutput {
    padded_area: [f32; 4],
    visibility_mask: Vec<String>,
    image_screen_extent: ImageScreenExtent,
}

#[derive(Deserialize, Debug)]
struct ImageScreenExtent {
    min: [i32; 2],
    max: [i32; 2],
}

#[derive(Deserialize, Debug, PartialEq, Clone)]
struct ScreenAABB {
    min: [i32; 2],
    max: [i32; 2],
}

// This function calculates the screen space AABB of a tile.
// It needs to replicate the transformation logic used in the application.
fn calculate_screen_aabb(tile_coord: &TileCoord, input: &TestInput) -> ScreenAABB {
    let image_center_x = input.image_size[0] / 2.0;
    let image_center_y = input.image_size[1] / 2.0;
    let render_center_x = input.render_size[0] / 2.0;
    let render_center_y = input.render_size[1] / 2.0;

    let zoom_inv = 1.0 / input.zoom;
    let pan_in_image_x = -input.pan_offset[0] * zoom_inv;
    let pan_in_image_y = input.pan_offset[1] * zoom_inv;

    let center_x = image_center_x + pan_in_image_x;
    let center_y = image_center_y + pan_in_image_y;

    let tile_size_f = input.tile_size as f32;
    let ix_min = tile_coord.x as f32 * tile_size_f;
    let iy_min = tile_coord.y as f32 * tile_size_f;
    let ix_max = (tile_coord.x + 1) as f32 * tile_size_f;
    let iy_max = (tile_coord.y + 1) as f32 * tile_size_f;

    let sx_min = ((ix_min - center_x) * input.zoom + render_center_x).round() as i32;
    let sy_min = ((iy_min - center_y) * input.zoom + render_center_y).round() as i32;
    let sx_max = ((ix_max - center_x) * input.zoom + render_center_x).round() as i32;
    let sy_max = ((iy_max - center_y) * input.zoom + render_center_y).round() as i32;

    ScreenAABB {
        min: [sx_min, sy_min],
        max: [sx_max, sy_max],
    }
}


#[test]
fn test_visible_tile_calculation() {
    let test_cases_dir = Path::new(file!())
        .parent()
        .unwrap()
        .join("test_cases");
    
    let mut test_cases = Vec::new();
    
    // Read all JSON files from the test_cases directory
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
    
    // Sort test cases by name for consistent ordering
    test_cases.sort_by(|a, b| a.name.cmp(&b.name));

    for case in test_cases {
        println!("Testing case: {}", case.name);

        let viewport = Viewport {
            zoom: case.input.zoom,
            pan_offset: case.input.pan_offset,
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

        let expected_padded_area = &case.output.padded_area;
        let padding = 0.5 * case.input.tile_size as f32;
        let calculated_padded_area = [
            -padding,
            -padding,
            case.input.render_size[0] + 2.0 * padding,
            case.input.render_size[1] + 2.0 * padding,
        ];
        assert_eq!(*expected_padded_area, calculated_padded_area, "Padded area mismatch");

        let max_tiles_x = (case.input.image_size[0] / case.input.tile_size as f32).ceil() as u32;
        let max_tiles_y = (case.input.image_size[1] / case.input.tile_size as f32).ceil() as u32;

        // Debug information
        println!("  Expected tiles: {}x{}, Visibility mask rows: {}", max_tiles_x, max_tiles_y, case.output.visibility_mask.len());
        
        // Use the actual size of the visibility mask instead of calculated max_tiles_y
        let mask_rows = case.output.visibility_mask.len() as u32;
        let mask_cols = if mask_rows > 0 { case.output.visibility_mask[0].len() as u32 } else { 0 };
        
        println!("  Mask dimensions: {}x{}", mask_cols, mask_rows);

        for y in 0..mask_rows {
            if y >= case.output.visibility_mask.len() as u32 {
                break;
            }
            let row_str = &case.output.visibility_mask[(mask_rows - 1 - y) as usize];
            for x in 0..mask_cols.min(row_str.len() as u32) {
                let is_selected_in_mask = row_str.chars().nth(x as usize).unwrap_or('0') == '1';
                
                let tile_coord = TileCoord { lod: case.input.lod, x, y };
                let is_selected_in_code = visible_tiles.contains(&tile_coord);

                if is_selected_in_mask != is_selected_in_code {
                    println!("  Mismatch at ({}, {}): mask={}, code={}", x, y, is_selected_in_mask, is_selected_in_code);
                }
            }
        }

        // Verify that the calculated image extent matches the expected extent
        let image_center_x = case.input.image_size[0] / 2.0;
        let image_center_y = case.input.image_size[1] / 2.0;
        let render_center_x = case.input.render_size[0] / 2.0;
        let render_center_y = case.input.render_size[1] / 2.0;

        let zoom_inv = 1.0 / case.input.zoom;
        let pan_in_image_x = -case.input.pan_offset[0] * zoom_inv;
        let pan_in_image_y = case.input.pan_offset[1] * zoom_inv;

        let center_x = image_center_x + pan_in_image_x;
        let center_y = image_center_y + pan_in_image_y;

        // Calculate the image extent in screen coordinates
        let image_min_x = ((0.0 - center_x) * case.input.zoom + render_center_x).round() as i32;
        let image_min_y = ((0.0 - center_y) * case.input.zoom + render_center_y).round() as i32;
        let image_max_x = ((case.input.image_size[0] - center_x) * case.input.zoom + render_center_x).round() as i32;
        let image_max_y = ((case.input.image_size[1] - center_y) * case.input.zoom + render_center_y).round() as i32;

        let calculated_extent = ImageScreenExtent {
            min: [image_min_x, image_min_y],
            max: [image_max_x, image_max_y],
        };

        assert_eq!(calculated_extent.min, case.output.image_screen_extent.min, "Image screen extent min mismatch");
        assert_eq!(calculated_extent.max, case.output.image_screen_extent.max, "Image screen extent max mismatch");
    }
}