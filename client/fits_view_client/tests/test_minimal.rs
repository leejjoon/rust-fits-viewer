use fits_view_client::Viewport;

#[test]
fn test_minimal_get_visible_tiles() {
    let mut viewport = Viewport::default();
    viewport.zoom = 1.0;
    viewport.pan_offset = egui::Vec2::new(-128.0, 0.0);
    
    let window_size = egui::Vec2::new(800.0, 600.0);
    let image_size = egui::Vec2::new(512.0, 512.0);
    
    println!("Testing with zoom={}, pan=[{}, {}]", viewport.zoom, viewport.pan_offset.x, viewport.pan_offset.y);
    
    let tiles = viewport.get_visible_tiles(window_size, image_size, 256);
    println!("Result: {} tiles", tiles.len());
    
    // This should work with zoom=1.0 and should show tiles
    assert!(tiles.len() > 0, "Should have visible tiles with zoom=1.0");
}
