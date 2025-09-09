use fits_view_client::test_data_validation::*;
use fits_view_client::coordinate_transform::*;
use std::path::Path;

#[test]
fn test_simple_no_zoom_no_pan() {
    let test_path = "tests/test_cases/simple_no_zoom_no_pan.json";
    if Path::new(test_path).exists() {
        let test_case = load_test_case(test_path).expect("Failed to load test case");
        validate_test_case(&test_case).expect("Test case validation failed");
    } else {
        println!("Test case file not found: {}", test_path);
    }
}

#[test]
fn test_larger_image_no_pan() {
    let test_path = "tests/test_cases/larger_image_no_pan.json";
    if Path::new(test_path).exists() {
        let test_case = load_test_case(test_path).expect("Failed to load test case");
        validate_test_case(&test_case).expect("Test case validation failed");
    } else {
        println!("Test case file not found: {}", test_path);
    }
}

#[test]
fn test_zoomed_in_view() {
    let test_path = "tests/test_cases/zoomed_in_view.json";
    if Path::new(test_path).exists() {
        let test_case = load_test_case(test_path).expect("Failed to load test case");
        validate_test_case(&test_case).expect("Test case validation failed");
    } else {
        println!("Test case file not found: {}", test_path);
    }
}

#[test]
fn test_panned_view() {
    let test_path = "tests/test_cases/panned_view.json";
    if Path::new(test_path).exists() {
        let test_case = load_test_case(test_path).expect("Failed to load test case");
        validate_test_case(&test_case).expect("Test case validation failed");
    } else {
        println!("Test case file not found: {}", test_path);
    }
}

#[test]
fn test_lod_1_view() {
    let test_path = "tests/test_cases/lod_1_view.json";
    if Path::new(test_path).exists() {
        let test_case = load_test_case(test_path).expect("Failed to load test case");
        validate_test_case(&test_case).expect("Test case validation failed");
    } else {
        println!("Test case file not found: {}", test_path);
    }
}

#[test]
fn test_all_generated_cases() {
    let test_dir = "tests/test_cases_generated";
    if Path::new(test_dir).exists() {
        match validate_all_test_cases(test_dir) {
            Ok(()) => println!("All generated test cases passed"),
            Err(e) => panic!("Generated test cases failed: {}", e),
        }
    } else {
        println!("Generated test cases directory not found: {}", test_dir);
    }
}

#[test]
fn test_lod_calculation_edge_cases() {
    // Test LOD calculation with various zoom levels
    assert_eq!(calculate_lod(1.0, 10), 0);   // zoom=1.0 -> LOD 0
    assert_eq!(calculate_lod(0.5, 10), 1);   // zoom=0.5 -> LOD 1  
    assert_eq!(calculate_lod(0.25, 10), 2);  // zoom=0.25 -> LOD 2
    assert_eq!(calculate_lod(0.125, 10), 3); // zoom=0.125 -> LOD 3
    assert_eq!(calculate_lod(2.0, 10), 0);   // zoom=2.0 -> LOD 0 (clamped)
    assert_eq!(calculate_lod(4.0, 10), 0);   // zoom=4.0 -> LOD 0 (clamped)
    
    // Test with max_lod constraint
    assert_eq!(calculate_lod(0.01, 2), 2);   // Would be LOD 6, clamped to 2
}

#[test]
fn test_max_lod_calculation() {
    // Test max LOD calculation with various image sizes
    let large_image = ImageSize { width: 2048.0, height: 2048.0 };
    assert_eq!(calculate_max_lod(large_image, 256), 3); // floor(log2(2048/256)) = 3
    
    let medium_image = ImageSize { width: 1024.0, height: 1024.0 };
    assert_eq!(calculate_max_lod(medium_image, 256), 2); // floor(log2(1024/256)) = 2
    
    let small_image = ImageSize { width: 512.0, height: 512.0 };
    assert_eq!(calculate_max_lod(small_image, 256), 1); // floor(log2(512/256)) = 1
    
    let tiny_image = ImageSize { width: 200.0, height: 200.0 };
    assert_eq!(calculate_max_lod(tiny_image, 256), 0); // Image smaller than tile
    
    // Test rectangular images (uses smaller dimension)
    let rect_image = ImageSize { width: 4096.0, height: 1024.0 };
    assert_eq!(calculate_max_lod(rect_image, 256), 2); // floor(log2(1024/256)) = 2
}

#[test]
fn test_screen_to_image_transform() {
    let mut viewport = Viewport::default();
    let render_size = RenderSize { width: 800.0, height: 600.0 };
    viewport.center_on_image = [400.0, 300.0];

    // Center of screen should map to center of image
    let (ix, iy) = screen_to_image(400.0, 300.0, &viewport, render_size);
    assert!((ix - 400.0).abs() < 1e-3);
    assert!((iy - 300.0).abs() < 1e-3);

    // Test with pan (center moved)
    viewport.center_on_image = [300.0, 250.0];
    let (ix_pan, iy_pan) = screen_to_image(400.0, 300.0, &viewport, render_size);
    assert!((ix_pan - 300.0).abs() < 1e-3);
    assert!((iy_pan - 250.0).abs() < 1e-3);
}