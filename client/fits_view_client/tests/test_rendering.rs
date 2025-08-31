use fits_view_client::*;

#[test]
fn test_initial_centering() {
    // Create a simple test app with mock data
    let meta = MetaResponse {
        shape: [1024, 1024],
        tile_size: 256,
        min_val: 0.0,
        max_val: 255.0,
    };
    
    // Test the viewport centering logic
    let mut viewport = Viewport::default();
    let window_size = egui::Vec2::new(800.0, 600.0);
    let image_size = egui::Vec2::new(meta.shape[0] as f32, meta.shape[1] as f32);
    viewport.fit_to_window(window_size, image_size);
    
    // Verify zoom is set correctly for fitting
    let expected_zoom = (800.0_f32 / 1024.0_f32).min(600.0_f32 / 1024.0_f32);
    assert!((viewport.zoom - expected_zoom).abs() < 0.001, 
        "Expected zoom {:.3}, got {:.3}", expected_zoom, viewport.zoom);
    
    // Verify pan offset is zero (centered)
    assert_eq!(viewport.pan_offset, egui::Vec2::ZERO);
    
    println!("✅ Initial centering test passed");
}

#[test]
fn test_tile_coordinate_calculation() {
    // Test the tile coordinate calculation logic
    let viewport = Viewport {
        zoom: 1.0,
        pan_offset: egui::Vec2::ZERO,
        rotation_angle: 0.0,
    };
    
    let render_size = egui::Vec2::new(800.0, 600.0);
    let image_size = egui::Vec2::new(1024.0, 1024.0);
    let tile_size = 256;
    
    let visible_tiles = viewport.get_visible_tiles(render_size, image_size, tile_size);
    
    // At zoom 1.0 with no pan, we should see all tiles
    assert_eq!(visible_tiles.len(), 16); // 4x4 grid
    
    // Verify we have tiles from (0,0) to (3,3)
    let mut coords: Vec<(u32, u32)> = visible_tiles.iter().map(|t| (t.x, t.y)).collect();
    coords.sort();
    
    let mut expected_coords = Vec::new();
    for x in 0..4 {
        for y in 0..4 {
            expected_coords.push((x, y));
        }
    }
    
    assert_eq!(coords, expected_coords);
}

#[test]
fn test_viewport_zoom_calculations() {
    // Test different zoom scenarios
    let mut viewport = Viewport::default();
    
    // Test fitting 1024x1024 image to 800x600 window
    viewport.fit_to_window(egui::Vec2::new(800.0, 600.0), egui::Vec2::new(1024.0, 1024.0));
    let expected_zoom = (800.0_f32 / 1024.0_f32).min(600.0_f32 / 1024.0_f32); // Should be limited by height
    assert!((viewport.zoom - expected_zoom).abs() < 0.001);
    
    // Test fitting 512x512 image to 800x600 window  
    viewport.fit_to_window(egui::Vec2::new(800.0, 600.0), egui::Vec2::new(512.0, 512.0));
    let expected_zoom = (800.0_f32 / 512.0_f32).min(600.0_f32 / 512.0_f32); // Should be 1.0 (no scaling needed)
    assert!((viewport.zoom - expected_zoom).abs() < 0.001);
    
    println!("✅ Viewport zoom calculations test passed");
}
