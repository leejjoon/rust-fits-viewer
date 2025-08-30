
---

# Phase 1 Technical Blueprint: FITS-View Prototype (wgpu & Binary Data Update)

## 1. Mission Statement & Goals

The goal of Phase 1 is to develop a functional, **high-performance FITS-View prototype** that validates the end-to-end architecture and leverages client-side GPU acceleration.

By the end of this phase:

* Users can display large NumPy arrays from FITS images in a desktop GUI.
* The GUI supports **fluid, GPU-accelerated panning, zooming, and rotation**.
* Two data transfer modes are supported:

  * **8-bit PNG** (low bandwidth, legacy path).
  * **RAW binary** (high-performance, for real-time contrast/colormap control).

* Refer "header_schema.md" document for header schema.

---

## 2. Core Technologies

* **Server-Side**

  * Language: Python 3.9+
  * Framework: FastAPI
  * Server: Uvicorn
  * Core Libraries: NumPy, Astropy (for FITS handling)

* **Client-Side**

  * Language: Rust
  * GUI Framework: `egui` + `eframe`
  * Rendering Engine: `wgpu` (cross-platform GPU API)
  * Integration: `egui_wgpu`
  * Shaders: WGSL (WebGPU Shading Language)
  * Core Crates: `tokio` (async runtime), `reqwest` (HTTP), `image` (PNG parsing), `bytemuck` (GPU-safe casting)

---

## 3. System Architecture

The client now takes on all rendering transformations, enabling dynamic interaction (rotation, stretch modes, colormaps).

### Data Flow (RAW Mode)

1. User calls `fits_view.show(array)` in Python.
2. FastAPI server starts in a background thread.
3. Rust client (`egui` + `wgpu`) launches.
4. Client requests `/tile/{z}/{x}/{y}?format=raw`.
5. Server slices the NumPy array, applies FITS scaling (BSCALE/BZERO), masks blanks, downsamples if needed, and converts to little-endian `float32`.
6. Response includes a **binary header** + tile data.
7. Client uploads the tile into a GPU `R32Float` texture.
8. WGSL shaders:

   * Vertex shader applies pan, zoom, rotation (around view center).
   * Fragment shader normalizes values with `(vmin,vmax)` and applies a 1D colormap texture.
   * NaN values map to a reserved colormap entry.
9. Result displayed in an `egui` window.

---

## 4. Server (Python / FastAPI)

### A. API Endpoints

* **`GET /meta`**

  * Returns JSON: array shape, dtype, tile size, zoom levels, global min/max, optional histogram bins.
* **`GET /tile/{z}/{x}/{y}`**

  * Query param: `format = png | raw`
  * Query param: `dtype` (optional, default `float32`)
  * Returns PNG or RAW tile.
* **`GET /cutout`** (optional Phase 1)

  * Extracts rectangular ROI with requested dtype.

### B. RAW Binary Header (32 bytes)

Before tile data, prepend a header:

```
[0..3]   u32 width
[4..7]   u32 height
[8]      u8 dtype_code   // 1=u8, 2=i16, 3=u16, 4=f32, 5=f64
[9]      u8 endianness   // 1=little, 2=big
[10]     u8 channels     // 1=scalar
[11]     u8 reserved
[12..15] u32 stride_bytes
[16..23] u64 tile_id
[24..31] reserved
```

HTTP headers may mirror these for debugging.

### C. FITS Handling

* Apply **BSCALE/BZERO**.
* Mask **BLANK/NaN**.
* Always return **little-endian float32** unless `dtype=` specified.
* Consistent orientation (document Y axis convention).

---

## 5. Client (Rust / `wgpu` + `egui`)

### A. Rendering Pipeline

1. Initialize `wgpu` adapter/device/queue/surface.
2. Create a `RenderPipeline` with WGSL shaders.
3. **Vertex Shader**: apply pan/zoom/rotation matrix around view center.
4. **Fragment Shader**: normalize values, apply colormap, branch NaN → special color.
5. Use `textureSampleLevel` with explicit LOD for smoother zoom-outs.

### B. Uniforms

Single uniform buffer (aligned):

* 4×4 transform matrix
* vmin, vmax, stretch\_mode
* nan\_color
* texel aspect

### C. Application State

```rust
struct Viewport {
    zoom: f32,
    pan_offset: egui::Vec2,
    rotation_angle: f32,
}

struct FitsViewApp {
    viewport: Viewport,
    tile_cache: TileCache,
    vmin: f32,
    vmax: f32,
    colormap: CurrentColormap,
    wgpu_render_state: Option<egui_wgpu::RenderState>,
}
```

### D. Input Mapping

* Mouse drag → pan
* Scroll → zoom
* Ctrl + Scroll → rotate
* `R` → reset view
* `F` → fit to window

### E. Tile Cache

* LRU eviction by GPU memory budget (e.g., 512 MB).
* Prefetch neighbors along motion vector.
* Cancel redundant requests on zoom change.

---

## 6. Testing Plan

* **Unit Tests**

  * Normalization functions (linear/log/sqrt/asinh).
  * Matrix generation (zoom/pan/rotation correctness).
* **Integration Tests**

  * Simulated egui interactions (mouse, sliders).
  * Mock server with deterministic gradients.
* **Visual Regression Tests**

  * Automated pan/zoom/rotate with screenshots.
  * Compare with golden images using **SSIM ≥ 0.995** (not pixel-exact).
  * Run across backends (Vulkan/DX12/Metal).
* **Performance Baseline**

  * Record FPS, p95 frame time. Alert on >20% regression.

---

## 7. Phase 1 Deliverables

1. Python package exposing `fits_view.show()`
2. Rust client desktop executable
3. Expanded test suite (unit, integration, visual regression)
4. Documentation:

   * `/meta`, `/tile`, `/cutout` API
   * PNG vs RAW trade-offs
   * Supported stretch modes (linear, log, sqrt, asinh)
   * Keyboard/mouse shortcuts

---

## 8. Future-Proofing (Beyond Phase 1)

* Add histograms & percentile auto-stretch.
* Multi-HDU / 3D cube slice browsing.
* WCS overlays (from FITS headers).
* Compression support (gzip/zstd for RAW).
* Persistent session save/restore.

---
