#!/usr/bin/env python3
"""
Test case generator for FITS viewer visible tile calculation tests.

This script generates JSON test data based on the coordinate transformation pipeline
described in fits_viewer_pipeline.md.
"""

import json
import math
from typing import List, Tuple, Dict, Any


class TestCaseGenerator:
    """
    Generates test cases for visible tile calculation based on the FITS viewer pipeline.
    
    The coordinate transformation follows this chain:
    1. Image Space → Screen Space (via pan, zoom, rotation)
    2. Screen Space → Padded Screen Space (add padding for tile loading)
    3. Padded Screen Space → Image Space (inverse transform)
    4. Image Space → Tile Coordinates (determine which tiles are needed)
    """
    
    def __init__(self, render_size: Tuple[float, float], image_size: Tuple[float, float], tile_size: int):
        """
        Initialize the test case generator.
        
        Args:
            render_size: (width, height) of the render viewport in pixels
            image_size: (width, height) of the image in pixels  
            tile_size: Size of each tile in pixels (assumed square)
        """
        self.render_size = render_size
        self.image_size = image_size
        self.tile_size = tile_size
        
    def generate_test_case(self, 
                          name: str,
                          zoom: float = 1.0, 
                          pan_offset: Tuple[float, float] = (0.0, 0.0),
                          lod: int = 0) -> Dict[str, Any]:
        """
        Generate a complete test case JSON object.
        
        Args:
            name: Descriptive name for the test case
            zoom: Zoom factor (1.0 = no zoom, >1.0 = zoomed in, <1.0 = zoomed out)
            pan_offset: (x, y) pan offset in normalized coordinates
            lod: Level of Detail (0 = full resolution, 1 = half resolution, etc.)
            
        Returns:
            Dictionary containing the complete test case data
        """
        
        # Calculate padded area (half tile size padding on each side)
        padding = self.tile_size * 0.5
        padded_area = [
            -padding,
            -padding, 
            self.render_size[0] + 2 * padding,
            self.render_size[1] + 2 * padding
        ]
        
        # Calculate visible tiles using the pipeline transformation
        visible_tiles = self._calculate_visible_tiles(zoom, pan_offset, lod)
        
        # Generate visibility mask
        visibility_mask = self._generate_visibility_mask(visible_tiles, lod)
        
        # Calculate image screen extent
        image_screen_extent = self._calculate_image_screen_extent(zoom, pan_offset)
        
        return {
            "name": name,
            "input": {
                "render_size": list(self.render_size),
                "image_size": list(self.image_size),
                "zoom": zoom,
                "pan_offset": list(pan_offset),
                "tile_size": self.tile_size,
                "lod": lod
            },
            "output": {
                "padded_area": padded_area,
                "visibility_mask": visibility_mask,
                "image_screen_extent": image_screen_extent
            }
        }
    
    def _calculate_visible_tiles(self, zoom: float, pan_offset: Tuple[float, float], lod: int) -> List[Tuple[int, int, int]]:
        """
        Calculate which tiles are visible based on the transformation pipeline.
        
        This follows the algorithm described in section 3 of fits_viewer_pipeline.md:
        1. Define padded screen area
        2. Transform corners to image space
        3. Calculate AABB in image space
        4. Convert to tile indices
        """
        
        # Step 1: Define padded screen area corners
        padding = self.tile_size * 0.5
        corners_screen = [
            (-padding, -padding),
            (self.render_size[0] + padding, -padding),
            (self.render_size[0] + padding, self.render_size[1] + padding),
            (-padding, self.render_size[1] + padding)
        ]
        
        # Step 2: Transform corners to image space
        corners_image = []
        for sx, sy in corners_screen:
            ix, iy = self._screen_to_image(sx, sy, zoom, pan_offset)
            corners_image.append((ix, iy))
        
        # Step 3: Calculate AABB in image space
        min_x = min(corner[0] for corner in corners_image)
        max_x = max(corner[0] for corner in corners_image)
        min_y = min(corner[1] for corner in corners_image)
        max_y = max(corner[1] for corner in corners_image)
        
        # Step 4: Convert to tile indices
        # For LOD > 0, the effective image size is smaller
        lod_scale = 2 ** lod
        effective_tile_size = self.tile_size * lod_scale
        
        min_tx = math.floor(min_x / effective_tile_size)
        max_tx = math.ceil(max_x / effective_tile_size)
        min_ty = math.floor(min_y / effective_tile_size)
        max_ty = math.ceil(max_y / effective_tile_size)
        
        # Generate list of visible tile coordinates
        visible_tiles = []
        for ty in range(min_ty, max_ty):
            for tx in range(min_tx, max_tx):
                # Ensure tiles are within image bounds
                if self._is_tile_in_bounds(tx, ty, lod):
                    visible_tiles.append((tx, ty, lod))
        
        return visible_tiles
    
    def _screen_to_image(self, sx: float, sy: float, zoom: float, pan_offset: Tuple[float, float]) -> Tuple[float, float]:
        """
        Transform screen coordinates to image coordinates.
        
        This is the inverse of the Image → Screen transformation described in section 2.3.
        """
        render_center_x = self.render_size[0] / 2.0
        render_center_y = self.render_size[1] / 2.0
        image_center_x = self.image_size[0] / 2.0
        image_center_y = self.image_size[1] / 2.0
        
        # Inverse zoom and pan calculations
        zoom_inv = 1.0 / zoom
        pan_in_image_x = -pan_offset[0] * zoom_inv
        pan_in_image_y = pan_offset[1] * zoom_inv
        
        center_x = image_center_x + pan_in_image_x
        center_y = image_center_y + pan_in_image_y
        
        # Transform screen to image coordinates
        ix = (sx - render_center_x) * zoom_inv + center_x
        iy = (sy - render_center_y) * zoom_inv + center_y
        
        return (ix, iy)
    
    def _is_tile_in_bounds(self, tx: int, ty: int, lod: int) -> bool:
        """Check if a tile coordinate is within the image bounds for the given LOD."""
        lod_scale = 2 ** lod
        effective_image_width = self.image_size[0] / lod_scale
        effective_image_height = self.image_size[1] / lod_scale
        effective_tile_size = self.tile_size
        
        max_tiles_x = math.ceil(effective_image_width / effective_tile_size)
        max_tiles_y = math.ceil(effective_image_height / effective_tile_size)
        
        return 0 <= tx < max_tiles_x and 0 <= ty < max_tiles_y
    
    def _generate_visibility_mask(self, visible_tiles: List[Tuple[int, int, int]], lod: int) -> List[str]:
        """
        Generate the visibility mask string representation.
        
        The mask shows which tiles are visible as a 2D grid of '1' (visible) and '0' (not visible).
        Rows are ordered from top to bottom (highest Y to lowest Y).
        """
        lod_scale = 2 ** lod
        effective_image_width = self.image_size[0] / lod_scale
        effective_image_height = self.image_size[1] / lod_scale
        effective_tile_size = self.tile_size
        
        max_tiles_x = math.ceil(effective_image_width / effective_tile_size)
        max_tiles_y = math.ceil(effective_image_height / effective_tile_size)
        
        # Create set of visible tile coordinates for fast lookup
        visible_set = {(tx, ty) for tx, ty, tz in visible_tiles}
        
        # Generate mask rows (top to bottom)
        mask_rows = []
        for ty in range(max_tiles_y - 1, -1, -1):  # Top to bottom
            row = ""
            for tx in range(max_tiles_x):
                if (tx, ty) in visible_set:
                    row += "1"
                else:
                    row += "0"
            mask_rows.append(row)
        
        return mask_rows
    
    def _calculate_image_screen_extent(self, zoom: float, pan_offset: Tuple[float, float]) -> Dict[str, List[int]]:
        """
        Calculate the extent of the image in screen coordinates.
        
        This represents the bounding box of the entire image when transformed to screen space.
        """
        render_center_x = self.render_size[0] / 2.0
        render_center_y = self.render_size[1] / 2.0
        image_center_x = self.image_size[0] / 2.0
        image_center_y = self.image_size[1] / 2.0
        
        zoom_inv = 1.0 / zoom
        pan_in_image_x = -pan_offset[0] * zoom_inv
        pan_in_image_y = pan_offset[1] * zoom_inv
        
        center_x = image_center_x + pan_in_image_x
        center_y = image_center_y + pan_in_image_y
        
        # Transform image corners to screen coordinates
        image_min_x = round((0.0 - center_x) * zoom + render_center_x)
        image_min_y = round((0.0 - center_y) * zoom + render_center_y)
        image_max_x = round((self.image_size[0] - center_x) * zoom + render_center_x)
        image_max_y = round((self.image_size[1] - center_y) * zoom + render_center_y)
        
        return {
            "min": [int(image_min_x), int(image_min_y)],
            "max": [int(image_max_x), int(image_max_y)]
        }


def main():
    """Generate the standard test cases."""
    
    # Test case 1: Simple case - image fits exactly in viewport
    generator1 = TestCaseGenerator(
        render_size=(800, 600),
        image_size=(800, 600), 
        tile_size=256
    )
    
    case1 = generator1.generate_test_case(
        name="Simple case: No zoom, no pan",
        zoom=1.0,
        pan_offset=(0.0, 0.0),
        lod=0
    )
    
    # Test case 2: Larger image, no pan
    generator2 = TestCaseGenerator(
        render_size=(800, 600),
        image_size=(2048, 2048),
        tile_size=256
    )
    
    case2 = generator2.generate_test_case(
        name="Larger image, no pan", 
        zoom=1.0,
        pan_offset=(0.0, 0.0),
        lod=0
    )
    
    # Test case 3: Zoomed in view
    case3 = generator2.generate_test_case(
        name="Zoomed-in view",
        zoom=2.0,
        pan_offset=(0.0, 0.0),
        lod=0
    )
    
    # Test case 4: Panned view
    case4 = generator2.generate_test_case(
        name="Panned view",
        zoom=1.0,
        pan_offset=(-0.5, -0.5),
        lod=0
    )
    
    # Test case 5: LOD 1 view
    case5 = generator2.generate_test_case(
        name="LOD 1 view",
        zoom=0.5,
        pan_offset=(0.0, 0.0),
        lod=1
    )
    
    # Save each test case to its own file
    test_cases = [
        ("simple_no_zoom_no_pan.json", case1),
        ("larger_image_no_pan.json", case2), 
        ("zoomed_in_view.json", case3),
        ("panned_view.json", case4),
        ("lod_1_view.json", case5)
    ]
    
    import os
    os.makedirs("client/fits_view_client/tests/test_cases_generated", exist_ok=True)
    
    for filename, case in test_cases:
        filepath = f"client/fits_view_client/tests/test_cases_generated/{filename}"
        with open(filepath, 'w') as f:
            json.dump(case, f, indent=2)
        print(f"Generated {filepath}")


if __name__ == "__main__":
    main()


