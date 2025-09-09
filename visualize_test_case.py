import json
import matplotlib.pyplot as plt
import matplotlib.patches as patches
import sys

def visualize_test_case(test_case, save_figure=False):
    """
    Visualizes a single test case from the visible_tiles_test_cases.json file.
    """
    input_data = test_case['input']
    output_data = test_case['output']
    
    render_width = input_data['render_size'][0]
    render_height = input_data['render_size'][1]

    fig, ax = plt.subplots(figsize=(10, 8))
    ax.set_aspect('equal', 'box')
    ax.set_title(test_case['name'])

    # Find the bounding box of all tiles to adjust the plot limits
    min_x_all = float('inf')
    min_y_all = float('inf')
    max_x_all = float('-inf')
    max_y_all = float('-inf')

    for tile in output_data['tiles']:
        aabb = tile['screen_aabb']
        min_x_all = min(min_x_all, aabb['min'][0])
        min_y_all = min(min_y_all, aabb['min'][1])
        max_x_all = max(max_x_all, aabb['max'][0])
        max_y_all = max(max_y_all, aabb['max'][1])

    # Add some padding to the limits
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

    # Draw the tiles using the visibility_mask
    visibility_mask = output_data['visibility_mask']
    num_rows = len(visibility_mask)
    num_cols = len(visibility_mask[0])

    for r, row_str in enumerate(visibility_mask):
        for c, char in enumerate(row_str):
            is_selected = (char == '1')
            
            # Find the corresponding tile in the tiles array to get screen_aabb
            # The visibility mask is top-to-bottom, so we invert the row index
            tile_y = num_rows - 1 - r
            tile_data = next((t for t in output_data['tiles'] if t['coord'][:2] == [c, tile_y]), None)
            if not tile_data:
                continue

            aabb = tile_data['screen_aabb']
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
            coord_text = f"({c}, {tile_y}, {tile_data['coord'][2]})"
            ax.text(min_x + width/2, min_y + height/2, coord_text, ha='center', va='center', fontsize=8)

    plt.xlabel("Screen X")
    plt.ylabel("Screen Y")
    plt.legend()
    
    if save_figure:
        # Save the figure
        filename = test_case['name'].replace(' ', '_').replace(',', '') + '.png'
        plt.savefig(filename)
        print(f"Saved figure to {filename}")
        plt.close(fig) # Close the figure to free up memory
    else:
        plt.show()


if __name__ == '__main__':
    save_figures = '--save' in sys.argv

    with open('client/fits_view_client/tests/visible_tiles_test_cases.json', 'r') as f:
        test_cases = json.load(f)
    
    for test_case in test_cases:
        visualize_test_case(test_case, save_figure=save_figures)
