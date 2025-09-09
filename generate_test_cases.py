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
    """
    
    def __init__(self, render_size: Tuple[float, float], image_size: Tuple[float, float], tile_size: int, max_lod: int = None):
        self.render_size = render_size
        self.image_size = image_size
        self.tile_size = tile_size
        self.max_lod = max_lod if max_lod is not None else self._calculate_max_lod()
        
    def generate_test_case(self, 
                          name: str,
                          zoom: float = 1.0, 
                          center_on_image: Tuple[float, float] = None) -> Dict[str, Any]:
        if center_on_image is None:
            center_on_image = (self.image_size[0] / 2.0, self.image_size[1] / 2.0)

        lod = self._calculate_lod(zoom)
        padding = self.tile_size * 0.5
        padded_area = [-padding, -padding, self.render_size[0] + 2 * padding, self.render_size[1] + 2 * padding]
        visible_tiles = self._calculate_visible_tiles(zoom, center_on_image, lod)
        visibility_mask = self._generate_visibility_mask(visible_tiles, lod)
        image_screen_extent = self._calculate_image_screen_extent(zoom, center_on_image)
        
        return {
            "name": name,
            "input": {
                "render_size": list(self.render_size),
                "image_size": list(self.image_size),
                "zoom": zoom,
                "center_on_image": list(center_on_image),
                "tile_size": self.tile_size
            },
            "output": {
                "lod": lod,
                "padded_area": padded_area,
                "visibility_mask": visibility_mask,
                "image_screen_extent": image_screen_extent
            }
        }
    
    def _calculate_visible_tiles(self, zoom: float, center_on_image: Tuple[float, float], lod: int) -> List[Tuple[int, int, int]]:
        padding = self.tile_size * 0.5
        corners_screen = [
            (-padding, -padding),
            (self.render_size[0] + padding, -padding),
            (self.render_size[0] + padding, self.render_size[1] + padding),
            (-padding, self.render_size[1] + padding)
        ]
        
        corners_image = [self._screen_to_image(sx, sy, zoom, center_on_image) for sx, sy in corners_screen]
        
        min_x = min(corner[0] for corner in corners_image)
        max_x = max(corner[0] for corner in corners_image)
        min_y = min(corner[1] for corner in corners_image)
        max_y = max(corner[1] for corner in corners_image)
        
        lod_scale = 2 ** lod
        effective_tile_size = self.tile_size * lod_scale
        
        min_tx = math.floor(min_x / effective_tile_size)
        max_tx = math.ceil(max_x / effective_tile_size)
        min_ty = math.floor(min_y / effective_tile_size)
        max_ty = math.ceil(max_y / effective_tile_size)
        
        visible_tiles = []
        for ty in range(min_ty, max_ty):
            for tx in range(min_tx, max_tx):
                if self._is_tile_in_bounds(tx, ty, lod):
                    visible_tiles.append((tx, ty, lod))
        
        return visible_tiles
    
    def _screen_to_image(self, sx: float, sy: float, zoom: float, center_on_image: Tuple[float, float]) -> Tuple[float, float]:
        ndc_x = (sx / self.render_size[0]) * 2.0 - 1.0
        ndc_y = (sy / self.render_size[1]) * 2.0 - 1.0

        scale_x = (2.0 * zoom) / self.render_size[0]
        scale_y = (2.0 * zoom) / self.render_size[1]

        ix = (ndc_x / scale_x) + center_on_image[0]
        iy = (ndc_y / scale_y) + center_on_image[1]

        return (ix, iy)

    def _calculate_max_lod(self) -> int:
        min_dim = min(self.image_size)
        if min_dim <= self.tile_size:
            return 0
        return max(0, int(math.floor(math.log2(min_dim / self.tile_size))))

    def _calculate_lod(self, zoom: float) -> int:
        if zoom <= 0:
            return self.max_lod
        z_ideal = -math.log2(zoom)
        return max(0, min(round(z_ideal), self.max_lod))

    def _is_tile_in_bounds(self, tx: int, ty: int, lod: int) -> bool:
        lod_scale = 2 ** lod
        max_tiles_x = math.ceil(self.image_size[0] / lod_scale / self.tile_size)
        max_tiles_y = math.ceil(self.image_size[1] / lod_scale / self.tile_size)
        return 0 <= tx < max_tiles_x and 0 <= ty < max_tiles_y

    def _generate_visibility_mask(self, visible_tiles: List[Tuple[int, int, int]], lod: int) -> List[str]:
        lod_scale = 2 ** lod
        max_tiles_x = math.ceil(self.image_size[0] / lod_scale / self.tile_size)
        max_tiles_y = math.ceil(self.image_size[1] / lod_scale / self.tile_size)
        visible_set = {(tx, ty) for tx, ty, _ in visible_tiles}
        
        mask_rows = []
        for ty in range(max_tiles_y - 1, -1, -1):
            row = "".join(["1" if (tx, ty) in visible_set else "0" for tx in range(max_tiles_x)])
            mask_rows.append(row)
        return mask_rows

    def _calculate_image_screen_extent(self, zoom: float, center_on_image: Tuple[float, float]) -> Dict[str, List[int]]:
        scale_x = (2.0 * zoom) / self.render_size[0]
        scale_y = (2.0 * zoom) / self.render_size[1]

        def image_to_screen(ix, iy):
            ndc_x = (ix - center_on_image[0]) * scale_x
            ndc_y = (iy - center_on_image[1]) * scale_y
            sx = (ndc_x + 1.0) * self.render_size[0] / 2.0
            sy = (ndc_y + 1.0) * self.render_size[1] / 2.0
            return round(sx), round(sy)

        min_sx, min_sy = image_to_screen(0, 0)
        max_sx, max_sy = image_to_screen(self.image_size[0], self.image_size[1])

        return {"min": [int(min_sx), int(min_sy)], "max": [int(max_sx), int(max_sy)]}

def main():
    """Generate the standard test cases."""
    generator = TestCaseGenerator(render_size=(800, 600), image_size=(2048, 2048), tile_size=256)
    
    cases = [
        ("simple_no_zoom_no_pan.json", generator.generate_test_case(name="Simple case: No zoom, no pan", zoom=0.3)),
        ("panned_view.json", generator.generate_test_case(name="Panned view", zoom=0.4, center_on_image=(1024, 512))),
        ("zoomed_in_view.json", generator.generate_test_case(name="Zoomed-in view", zoom=1.5, center_on_image=(1200, 1200))),
    ]
    
    import os
    output_dir = "client/fits_view_client/tests/test_cases_generated"
    os.makedirs(output_dir, exist_ok=True)
    
    for filename, case_data in cases:
        with open(os.path.join(output_dir, filename), 'w') as f:
            json.dump(case_data, f, indent=2)
        print(f"Generated {filename}")

if __name__ == "__main__":
    main()