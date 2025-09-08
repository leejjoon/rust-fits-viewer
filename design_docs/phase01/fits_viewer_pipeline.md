# FITS Viewer Coordinate System Transformations (Updated)

This document describes the coordinate transforms used in the FITS Viewer pipeline. It includes clarifications and fixes from the original draft, plus testing notes to ensure correctness when implementing in Rust with `wgpu`/`egui`.

---

## 1. Conventions

- **Global convention:** Y-up across all spaces (image, tile, NDC, clip). 
- **Tile index origin:** Bottom-left (consistent with Y-up). If the server uses top-left (e.g., XYZ scheme), an adapter layer must invert Y.
- **Texture coordinates:** Using **WebGPU/WGPU convention** — `(0,0)` is **top-left** of the texture. (This is different from legacy OpenGL.)
- **Matrices:** Column-major; vectors are column vectors; transformations are applied as `M * v`.

---

## 2. Transform Chain and Coordinate Spaces

To make development more intuitive, we clarify the coordinate spaces and the transformations between them.

### 2.1. Core Coordinate Spaces

1.  **Image Space:** The raw pixel grid of the FITS file. `(0,0)` is the bottom-left of the image data, Y is up.
2.  **Tile Space:** A conceptual partitioning of Image Space for data loading. A coordinate consists of a `(tile_x, tile_y)` index and a local pixel coordinate `(lx, ly)` within that tile.
3.  **NDC (Normalized Device Coordinates):** A `wgpu`-native space where the visible area is a cube from `(-1, -1, z_min)` to `(1, 1, z_max)`. This is the target space for our vertex shader.
4.  **Screen Space:** An intuitive 2D space corresponding to the render window's pixels. `(0,0)` is at the **bottom-left** of the window (to maintain the Y-up convention), with X pointing right and Y pointing up. `(render_width, render_height)` is the top-right corner.

### 2.2. Screen Space ↔ NDC Transformation

This conversion is key for mapping user input (e.g., mouse clicks in pixels) to the GPU's coordinate system, and vice-versa.

-   **Screen to NDC:**
    ```
    ndc_x = (2.0 * sx / render_width) - 1.0
    ndc_y = (2.0 * sy / render_height) - 1.0
    ```
-   **NDC to Screen:**
    ```
    sx = (ndc_x + 1.0) * render_width / 2.0
    sy = (ndc_y + 1.0) * render_height / 2.0
    ```

### 2.3. The Rendering Transform Chain

The core of the viewer is a single matrix, `M_image_to_ndc`, that transforms a vertex from image space directly to NDC. This is calculated on the CPU and sent to the GPU as a uniform.

**Chain:** `Image Space → NDC → Clip Space`

1.  **Image space → Tile space (for data loading)**
    - This is a conceptual split for determining which tiles are visible. It is not part of the primary rendering matrix.
    - Mapping:
      ```
      tile_x = floor(x / tile_size)
      lx     = x % tile_size
      ```
    - Reversible: `(x, y) = tile_x*TS + lx, tile_y*TS + ly`.

2.  **Image space → NDC (The `M_image_to_ndc` Matrix)**
    - This matrix combines pan, zoom, and rotation. Let `(ix, iy)` be a vertex in image space. The transformation steps are:
      1.  **Translate to Image Center:** Translate the image so its center `(image_width/2, image_height/2)` is at the origin.
      2.  **Apply Rotation (θ):** Rotate around the origin.
      3.  **Apply Pan:** Translate by `(pan_x, pan_y)`. Pan is applied in a space where 1 unit equals 1 image pixel at zoom=1.
      4.  **Apply Uniform Zoom (s):** Scale uniformly around the origin.
      5.  **Scale to NDC:** Scale the result to fit the `[-1, 1]` NDC range. This is done by dividing the X-coordinates by `render_width/2` and Y-coordinates by `render_height/2`.

    - The combined matrix `M_image_to_ndc` is what the vertex shader receives to transform vertices.

3.  **NDC → Clip space**
    - In `wgpu`, the vertex shader's output is in Clip Space. For our 2D rendering, NDC and Clip Space are effectively the same. The GPU pipeline takes the NDC output from the vertex shader and proceeds to rasterization.

---

## 3. Visible Tile Calculation

- Compute the **viewport corners in NDC**: `(-1,-1), (1,-1), (1,1), (-1,1)`.
- Transform each through `M_inv` (inverse of image→NDC) to get 4 points in image space.
- Take the **AABB (axis-aligned bounding box)** of those 4 points.
- Convert bounds to tile indices:
  ```
  min_tx = floor(min_x / tile_size)
  max_tx = floor(max_x / tile_size)
  ```
- Add **1-tile padding** to avoid popping during slow pan/rotation.

---

## 4. LOD Selection (Tile Zoom Level)

To optimize performance and visual fidelity, the viewer must request tiles at an appropriate resolution or Level of Detail (LOD) based on the current zoom. This prevents loading high-resolution data when zoomed out and avoids blurry upscaling when zoomed in.

### 4.1. Defining LOD Levels

We define a series of LODs, where `LOD 0` is the original, full-resolution image. Each subsequent level, `LOD z`, represents the image downsampled by a factor of `2^z`.

-   **LOD 0:** 1:1 scale (original resolution).
-   **LOD 1:** 1:2 scale (image is downsampled by a factor of 2).
-   **LOD 2:** 1:4 scale (image is downsampled by a factor of 4).
-   ...
-   **LOD z:** 1:2^z scale.

This strategy requires the server to be capable of generating and serving tiles for these different `z` levels.

### 4.2. Mapping View Zoom to the Ideal LOD

The zoom factor `s` from the `M_image_to_ndc` matrix directly represents how many screen pixels a single `LOD 0` image pixel covers. We want to select an `LOD z` where one pixel from that LOD's tiles appears as roughly one pixel on the screen.

A pixel in an `LOD z` tile corresponds to `2^z` pixels in the original `LOD 0` image. When rendered, this `LOD z` pixel will cover `s * 2^z` screen pixels. To make one tile pixel map to one screen pixel, we set this target to 1:

`s * 2^z = 1`

Solving for `z` gives us the ideal (fractional) LOD level:

`z_ideal = log2(1/s) = -log2(s)`

To get the concrete LOD level to request from the server, we can round this value to the nearest integer:

`lod_to_request = round(z_ideal)`

This `lod_to_request` value is then used when fetching tiles from the server.

### 4.3. Preventing Flickering with Hysteresis

Using `round()` directly can cause the requested LOD to flicker rapidly between two levels when the zoom is near a halfway point (e.g., when `z_ideal` is 1.5, 2.5, etc.). To solve this, we introduce a **hysteresis band**.

-   **Rule:** Only change the current `lod` if `abs(z_ideal - current_lod) > 0.5 + threshold`. A `threshold` of `0.2` or `0.3` is a good starting point.

This creates a "dead zone" where the LOD remains stable, preventing distracting and inefficient tile re-fetching during minor zoom adjustments.

---

## 5. Input Handling (Zoom to Cursor)

- Convert cursor `(sx,sy)` in screen coordinates → NDC → image coordinates (via `M_inv`).
- On zoom event, compute new scale `s'`.
- Adjust pan so that the same image point maps back to `(sx,sy)`.

This requires consistent use of the **inverse matrix**.

---

## 6. Extensions (FITS WCS)

- Current chain assumes square pixels.
- To support FITS WCS: insert one extra affine transform from **world → image** before the tile split.

---

## 7. Testing Strategy (Rust + wgpu)

### Image → Tile
- **Unit:** `(0,0)` → `(0,0)` local `(0,0)`.
- **Property-based:** `(tile, local)` roundtrip yields original `(x,y)`.
- **Edge case:** Non-multiple image sizes.

### Tile → Image → NDC
- **Known point:** Center of image → (0,0) NDC.
- **Inverse test:** `M_inv(M(p)) ≈ p`.
- **Zoom invariant:** After zoom-to-cursor, same image point remains under cursor.

### Shader Vertex Transform
- **GPU test:** Render 2×2 tex with colored quadrants; verify UV orientation matches WebGPU convention.
- **Seam test:** Render grid of flat-colored tiles; check no background lines at seams.

### Visible Tiles
- **Oracle test:** Brute-force overlap vs. index math; must agree.
- **Rotation:** Validate rotated viewport still fetches enough tiles.

### Input Handling
- **Drag test:** Positive Y drag moves image down.
- **Scroll test:** Zoom increments scale consistently across viewport sizes.

---

## 8. Notes on Robustness
- Precision: use `f64` CPU-side, `f32` GPU-side.
- Support very large images (≥32k px).
- Test fractional scales (1.25, 1.5, 2.0) on HiDPI.
- Consider gutters or overlap to prevent seams with linear filtering.

---

## 9. Summary

This pipeline description is corrected to:
- Use WebGPU texture convention.
- Clarify column-major matrix order.
- Make tile visibility rotation-aware.
- Add inverse matrix reuse for input.
- Define testing strategies for each stage.

With these refinements, the FITS Viewer architecture should be robust, predictable, and easier to validate with automated tests.