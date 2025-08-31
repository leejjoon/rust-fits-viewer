use fits_view_client::Viewport;

#[test]
fn test_debug_pan_calculation() {
    println!("🔧 Debug Pan Calculation Test");
    println!("=============================");
    
    let mut viewport = Viewport::default();
    let window_size = egui::Vec2::new(800.0, 600.0);
    let image_size = egui::Vec2::new(512.0, 512.0);
    
    viewport.fit_to_window(window_size, image_size);
    println!("Initial zoom: {:.3}", viewport.zoom);
    
    // Test with a simple pan offset
    viewport.pan_offset = egui::Vec2::new(-128.0, 0.0);
    
    // Manual calculation to debug
    let zoom_inv = 1.0 / viewport.zoom;
    let pan_in_image_x = -viewport.pan_offset.x * zoom_inv;
    let pan_in_image_y = viewport.pan_offset.y * zoom_inv;
    let image_center = image_size * 0.5;
    let center_x = image_center.x + pan_in_image_x;
    let center_y = image_center.y + pan_in_image_y;
    
    println!("Manual calculation:");
    println!("  Zoom: {:.3}, zoom_inv: {:.3}", viewport.zoom, zoom_inv);
    println!("  Pan offset: [{:.1}, {:.1}]", viewport.pan_offset.x, viewport.pan_offset.y);
    println!("  Pan in image: [{:.1}, {:.1}]", pan_in_image_x, pan_in_image_y);
    println!("  Image center: [{:.1}, {:.1}]", image_center.x, image_center.y);
    println!("  View center: [{:.1}, {:.1}]", center_x, center_y);
    
    // Calculate visible area bounds
    let half_render = window_size * 0.5;
    let visible_half_width = half_render.x * zoom_inv;
    let visible_half_height = half_render.y * zoom_inv;
    let padding_factor = 1.5;
    let expanded_half_width = visible_half_width * padding_factor;
    let expanded_half_height = visible_half_height * padding_factor;
    
    let min_x = (center_x - expanded_half_width).max(0.0);
    let max_x = (center_x + expanded_half_width).min(image_size.x);
    let min_y = (center_y - expanded_half_height).max(0.0);
    let max_y = (center_y + expanded_half_height).min(image_size.y);
    
    println!("  Visible half size: [{:.1}, {:.1}]", visible_half_width, visible_half_height);
    println!("  Expanded half size: [{:.1}, {:.1}]", expanded_half_width, expanded_half_height);
    println!("  Visible bounds: [{:.1}, {:.1}] to [{:.1}, {:.1}]", min_x, min_y, max_x, max_y);
    println!("  Bounds valid: {}", min_x < max_x && min_y < max_y);
    
    // Calculate tile coordinates
    let tile_size_f = 256.0;
    let min_tile_x = (min_x / tile_size_f).floor() as u32;
    let max_tile_x = (max_x / tile_size_f).ceil() as u32;
    let min_tile_y = (min_y / tile_size_f).floor() as u32;
    let max_tile_y = (max_y / tile_size_f).ceil() as u32;
    
    let max_tiles_x = (image_size.x as u32 + 256 - 1) / 256;
    let max_tiles_y = (image_size.y as u32 + 256 - 1) / 256;
    
    println!("  Tile range: x[{} to {}), y[{} to {})", min_tile_x, max_tile_x, min_tile_y, max_tile_y);
    println!("  Max tiles: x={}, y={}", max_tiles_x, max_tiles_y);
    
    // Simulate the loop logic
    let mut expected_tiles = Vec::new();
    for y in min_tile_y..max_tile_y {
        for x in min_tile_x..max_tile_x {
            println!("    Checking tile ({}, {}): x < {} && y < {} = {}", 
                     x, y, max_tiles_x, max_tiles_y, x < max_tiles_x && y < max_tiles_y);
            if x < max_tiles_x && y < max_tiles_y {
                expected_tiles.push((x, y));
            }
        }
    }
    println!("  Expected {} tiles: {:?}", expected_tiles.len(), expected_tiles);
    
    // Test the bounds calculation directly
    let zoom_inv = 1.0 / viewport.zoom;
    let visible_half_width = (window_size.x * 0.5) * zoom_inv;
    let visible_half_height = (window_size.y * 0.5) * zoom_inv;
    let pan_in_image_x = -viewport.pan_offset.x * zoom_inv;
    let pan_in_image_y = viewport.pan_offset.y * zoom_inv;
    let center_x = 256.0 + pan_in_image_x;
    let center_y = 256.0 + pan_in_image_y;
    
    let padding_factor = 1.5;
    let expanded_half_width = visible_half_width * padding_factor;
    let expanded_half_height = visible_half_height * padding_factor;
    
    let min_x = (center_x - expanded_half_width).max(0.0);
    let max_x = (center_x + expanded_half_width).min(512.0);
    let min_y = (center_y - expanded_half_height).max(0.0);
    let max_y = (center_y + expanded_half_height).min(512.0);
    
    println!("🔧 Direct bounds check:");
    println!("   center: [{:.1}, {:.1}]", center_x, center_y);
    println!("   expanded half: [{:.1}, {:.1}]", expanded_half_width, expanded_half_height);
    println!("   bounds: [{:.1}, {:.1}] to [{:.1}, {:.1}]", min_x, min_y, max_x, max_y);
    println!("   bounds valid: {}", min_x < max_x && min_y < max_y);
    
    println!("🔧 Calling get_visible_tiles...");
    let visible_tiles = viewport.get_visible_tiles(window_size, image_size, 256);
    println!("🔧 Result: {} visible tiles: {:?}", visible_tiles.len(), visible_tiles);
    
    // Test with no pan
    viewport.pan_offset = egui::Vec2::new(0.0, 0.0);
    let visible_tiles_zero = viewport.get_visible_tiles(window_size, image_size, 256);
    
    println!("Pan [0, 0] -> {} visible tiles", visible_tiles_zero.len());
}
