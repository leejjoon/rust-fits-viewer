import json
import matplotlib.pyplot as plt
import matplotlib.patches as patches
import sys
import os
import glob

def calculate_tile_screen_aabb(tile_x, tile_y, tile_z, input_data):
    """Calculate screen AABB for a tile given its coordinates and input parameters."""
    image_center_x = input_data['image_size'][0] / 2.0
    image_center_y = input_data['image_size'][1] / 2.0
    render_center_x = input_data['render_size'][0] / 2.0
    render_center_y = input_data['render_size'][1] / 2.0

    zoom_inv = 1.0 / input_data['zoom']
    pan_in_image_x = -input_data['pan_offset'][0] * zoom_inv
    pan_in_image_y = input_data['pan_offset'][1] * zoom_inv

    center_x = image_center_x + pan_in_image_x
    center_y = image_center_y + pan_in_image_y

    tile_size_f = input_data['tile_size']
    ix_min = tile_x * tile_size_f
    iy_min = tile_y * tile_size_f
    ix_max = (tile_x + 1) * tile_size_f
    iy_max = (tile_y + 1) * tile_size_f

    sx_min = int(round((ix_min - center_x) * input_data['zoom'] + render_center_x))
    sy_min = int(round((iy_min - center_y) * input_data['zoom'] + render_center_y))
    sx_max = int(round((ix_max - center_x) * input_data['zoom'] + render_center_x))
    sy_max = int(round((iy_max - center_y) * input_data['zoom'] + render_center_y))

    return {
        'min': [sx_min, sy_min],
        'max': [sx_max, sy_max]
    }

def visualize_test_case(test_case, save_figure=False, fig=None):
    """
    Visualizes a single test case from the test case JSON files.
    """
    input_data = test_case['input']
    output_data = test_case['output']
    
    render_width = input_data['render_size'][0]
    render_height = input_data['render_size'][1]

    if fig is None:
        fig, ax = plt.subplots(figsize=(10, 8))
    else:
        ax = fig.add_subplot(111)

    ax.set_aspect('equal', 'box')
    ax.set_title(test_case['name'])

    # Calculate plot limits by finding the extent of all tiles (visible and non-visible)
    visibility_mask = output_data['visibility_mask']
    num_rows = len(visibility_mask)
    num_cols = len(visibility_mask[0])
    
    # Calculate bounds from all possible tiles
    min_x_all = float('inf')
    min_y_all = float('inf')
    max_x_all = float('-inf')
    max_y_all = float('-inf')
    
    for r, row_str in enumerate(visibility_mask):
        for c, char in enumerate(row_str):
            # The visibility mask is top-to-bottom, so we invert the row index
            tile_y = num_rows - 1 - r
            tile_x = c
            tile_z = input_data['lod']
            
            # Calculate the screen AABB for this tile
            aabb = calculate_tile_screen_aabb(tile_x, tile_y, tile_z, input_data)
            min_x, min_y = aabb['min']
            max_x, max_y = aabb['max']
            
            min_x_all = min(min_x_all, min_x)
            min_y_all = min(min_y_all, min_y)
            max_x_all = max(max_x_all, max_x)
            max_y_all = max(max_y_all, max_y)
    
    # Also include the padded area bounds
    padded_area = output_data['padded_area']
    padded_min_x = padded_area[0]
    padded_min_y = padded_area[1]
    padded_max_x = padded_area[0] + padded_area[2]
    padded_max_y = padded_area[1] + padded_area[3]
    
    # Use the union of tile extents and padded area
    min_x_all = min(min_x_all, padded_min_x)
    min_y_all = min(min_y_all, padded_min_y)
    max_x_all = max(max_x_all, padded_max_x)
    max_y_all = max(max_y_all, padded_max_y)

    # Add some padding to the limits for better visualization
    padding_display = 100
    ax.set_xlim(min_x_all - padding_display, max_x_all + padding_display)
    ax.set_ylim(min_y_all - padding_display, max_y_all + padding_display)

    # Draw the render screen
    screen = patches.Rectangle((0, 0), render_width, render_height, linewidth=2, edgecolor='black', facecolor='none', label='Render Screen')
    ax.add_patch(screen)

    # Draw the padded area from the test data
    padded_area_data = output_data['padded_area']
    padded_x, padded_y, padded_width, padded_height = padded_area_data
    padded_area = patches.Rectangle((padded_x, padded_y), padded_width, padded_height, linewidth=2, linestyle='--', edgecolor='blue', facecolor='none', label='Padded Area')
    ax.add_patch(padded_area)

    # Draw the image screen extent
    image_extent = output_data['image_screen_extent']
    extent_min_x, extent_min_y = image_extent['min']
    extent_max_x, extent_max_y = image_extent['max']
    extent_width = extent_max_x - extent_min_x
    extent_height = extent_max_y - extent_min_y
    image_extent_rect = patches.Rectangle((extent_min_x, extent_min_y), extent_width, extent_height, linewidth=2, linestyle='-.', edgecolor='red', facecolor='none', label='Image Screen Extent')
    ax.add_patch(image_extent_rect)

    # Draw the tiles using the visibility_mask
    visibility_mask = output_data['visibility_mask']
    num_rows = len(visibility_mask)
    num_cols = len(visibility_mask[0])

    for r, row_str in enumerate(visibility_mask):
        for c, char in enumerate(row_str):
            is_selected = (char == '1')
            
            # The visibility mask is top-to-bottom, so we invert the row index
            tile_y = num_rows - 1 - r
            tile_x = c
            tile_z = input_data['lod']
            
            # Calculate the screen AABB for this tile
            aabb = calculate_tile_screen_aabb(tile_x, tile_y, tile_z, input_data)
            min_x, min_y = aabb['min']
            max_x, max_y = aabb['max']
            width = max_x - min_x
            height = max_y - min_y
            
            if is_selected:
                face_color = 'green'
                edge_color = 'darkgreen'
            else:
                face_color = 'red'
                edge_color = 'darkred'
                
            rect = patches.Rectangle((min_x, min_y), width, height, linewidth=1, edgecolor=edge_color, facecolor=face_color, alpha=0.5)
            ax.add_patch(rect)
            
            # Add tile coordinates text
            coord_text = f"({tile_x}, {tile_y}, {tile_z})"
            ax.text(min_x + width/2, min_y + height/2, coord_text, ha='center', va='center', fontsize=8)

    ax.set_xlabel("Screen X")
    ax.set_ylabel("Screen Y")
    ax.legend()
    
    if save_figure:
        # Save the figure
        filename = test_case['name'].replace(' ', '_').replace(',', '') + '.png'
        fig.savefig(filename)
        print(f"Saved figure to {filename}")
        plt.close(fig) # Close the figure to free up memory
    else:
        plt.show()


if __name__ == '__main__':
    save_figures = '--save' in sys.argv

    # Read all JSON files from the test_cases directory
    test_cases_dir = 'client/fits_view_client/tests/test_cases'
    json_files = glob.glob(os.path.join(test_cases_dir, '*.json'))
    
    test_cases = []
    for json_file in json_files:
        with open(json_file, 'r') as f:
            test_case = json.load(f)
            test_cases.append(test_case)
    
    # Sort test cases by name for consistent ordering
    test_cases.sort(key=lambda x: x['name'])
    
    for test_case in test_cases:
        visualize_test_case(test_case, save_figure=save_figures)
