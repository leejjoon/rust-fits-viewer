Great—here’s a tight, shared RAW-tile header schema with **exactly 32 bytes**, plus drop-in code for **Python (FastAPI)** and **Rust (client)** that serialize/deserialize the same structure safely.

---

# RAW Tile Header (32 bytes, little-endian)

**Layout (LE):**

```
offset  size  field           type
0       4     width           u32
4       4     height          u32
8       1     dtype_code      u8    // see enum below
9       1     endianness      u8    // 1=little, 2=big
10      1     channels        u8    // 1=scalar (Phase 1)
11      1     reserved0       u8    // =0
12      4     stride_bytes    u32   // 0 = tightly packed
16      8     tile_id         u64   // e.g., (z<<40) | (x<<20) | y
24      8     reserved1       u64   // =0
```

**Conventions (Phase 1):**

* Server sends **little-endian** `float32` payloads → `dtype_code=4`, `endianness=1`.
* `channels=1`, `stride_bytes=0` (tightly packed).
* PNG path doesn’t use this header (PNG bytes only).
* Future-proof: non-1 channels and non-0 stride allowed later.

**dtype\_code enum:**

```
1 = u8, 2 = i16, 3 = u16, 4 = f32, 5 = f64
```

---

# Python (FastAPI server)

## 1) Dataclass + struct format (exact 32 bytes)

```python
# server/header.py
from dataclasses import dataclass
import struct

# Little-endian, packed:
# <  = little-endian
# I  = u32 (width)
# I  = u32 (height)
# B  = u8  (dtype_code)
# B  = u8  (endianness)
# B  = u8  (channels)
# B  = u8  (reserved0)
# I  = u32 (stride_bytes)
# Q  = u64 (tile_id)
# Q  = u64 (reserved1)
RAW_HEADER_FMT = "<IIBBBBIQQ"
RAW_HEADER_SIZE = struct.calcsize(RAW_HEADER_FMT)
assert RAW_HEADER_SIZE == 32, RAW_HEADER_SIZE

DT_U8, DT_I16, DT_U16, DT_F32, DT_F64 = 1, 2, 3, 4, 5
ENDIAN_LITTLE, ENDIAN_BIG = 1, 2

@dataclass
class RawTileHeader:
    width: int
    height: int
    dtype_code: int
    endianness: int
    channels: int
    reserved0: int
    stride_bytes: int
    tile_id: int
    reserved1: int = 0

    def to_bytes(self) -> bytes:
        return struct.pack(
            RAW_HEADER_FMT,
            self.width,
            self.height,
            self.dtype_code,
            self.endianness,
            self.channels,
            self.reserved0,
            self.stride_bytes,
            self.tile_id,
            self.reserved1,
        )

    @classmethod
    def from_bytes(cls, b: bytes) -> "RawTileHeader":
        if len(b) < RAW_HEADER_SIZE:
            raise ValueError("header too short")
        width, height, dtype_code, endianness, channels, reserved0, stride_bytes, tile_id, reserved1 = struct.unpack(
            RAW_HEADER_FMT, b[:RAW_HEADER_SIZE]
        )
        return cls(width, height, dtype_code, endianness, channels, reserved0, stride_bytes, tile_id, reserved1)
```

## 2) Emit RAW: FITS → NumPy → float32 → header+payload

```python
# server/routes.py
from fastapi import APIRouter, HTTPException
from fastapi.responses import StreamingResponse
import numpy as np
from .header import RawTileHeader, RAW_HEADER_SIZE, DT_F32, ENDIAN_LITTLE

router = APIRouter()

# Assume you have: get_tile_numpy(z, x, y) -> np.ndarray (H, W), already sliced and downsampled,
# with BSCALE/BZERO applied and BLANK masked to NaN.

@router.get("/tile/{z}/{x}/{y}")
def get_tile(z: int, x: int, y: int, format: str = "png", dtype: str = "float32"):
    if format == "png":
        # ... your existing PNG path ...
        raise HTTPException(501, "PNG path not shown here")
    elif format == "raw":
        arr = get_tile_numpy(z, x, y)  # (H, W), float32 target
        # Phase 1: enforce LE float32
        if arr.dtype != np.float32:
            arr = arr.astype(np.float32, copy=False)
        if arr.dtype.byteorder not in ('<', '=', '|'):
            arr = arr.byteswap().newbyteorder('<')

        h, w = arr.shape[:2]
        payload = arr.tobytes(order="C")
        header = RawTileHeader(
            width=w,
            height=h,
            dtype_code=DT_F32,
            endianness=ENDIAN_LITTLE,
            channels=1,
            reserved0=0,
            stride_bytes=0,
            tile_id=((z & 0xFFFFF) << 40) | ((x & 0xFFFFF) << 20) | (y & 0xFFFFF),
        ).to_bytes()

        def gen():
            yield header
            yield payload

        return StreamingResponse(gen(), media_type="application/octet-stream", headers={
            "X-FV-Width": str(w),
            "X-FV-Height": str(h),
            "X-FV-DType": "f32",
            "X-FV-Endianness": "little",
        })
    else:
        raise HTTPException(400, "unknown format")
```

---

# Rust (client)

## 1) Shared struct (exact 32 bytes) with bytemuck

```rust
// client/src/raw_header.rs
use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Clone, Copy, Zeroable, Pod, Debug)]
pub struct RawTileHeader {
    pub width: u32,         // 0..4
    pub height: u32,        // 4..8
    pub dtype_code: u8,     // 8
    pub endianness: u8,     // 9
    pub channels: u8,       // 10
    pub reserved0: u8,      // 11
    pub stride_bytes: u32,  // 12..16
    pub tile_id: u64,       // 16..24
    pub reserved1: u64,     // 24..32
}

pub const RAW_HEADER_SIZE: usize = core::mem::size_of::<RawTileHeader>();
pub const DT_U8: u8 = 1;
pub const DT_I16: u8 = 2;
pub const DT_U16: u8 = 3;
pub const DT_F32: u8 = 4;
pub const DT_F64: u8 = 5;

pub const ENDIAN_LITTLE: u8 = 1;
pub const ENDIAN_BIG: u8 = 2;

impl RawTileHeader {
    pub fn from_prefix(bytes: &[u8]) -> Result<(Self, &[u8]), &'static str> {
        if bytes.len() < RAW_HEADER_SIZE { return Err("header too short"); }
        let (head_bytes, rest) = bytes.split_at(RAW_HEADER_SIZE);
        // SAFETY: bytemuck::from_bytes requires correct alignment & size; repr(C)+Pod guarantees it.
        let header: &RawTileHeader = bytemuck::from_bytes(head_bytes);
        Ok((*header, rest))
    }
}
```

Static size sanity check (optional):

```rust
const _: () = assert!(RAW_HEADER_SIZE == 32);
```

## 2) Parsing & payload checks

```rust
// client/src/parse.rs
use crate::raw_header::{RawTileHeader, RAW_HEADER_SIZE, DT_F32, ENDIAN_LITTLE};

pub struct RawTile<'a> {
    pub header: RawTileHeader,
    pub payload: &'a [u8],
}

pub fn parse_raw_tile(bytes: &[u8]) -> Result<RawTile, String> {
    let (hdr, payload) = RawTileHeader::from_prefix(bytes).map_err(|e| e.to_string())?;

    // Phase 1 constraints:
    if hdr.dtype_code != DT_F32 { return Err("unsupported dtype (expect f32)".into()); }
    if hdr.endianness != ENDIAN_LITTLE { return Err("unsupported endianness (expect little)".into()); }
    if hdr.channels != 1 { return Err("unsupported channels (expect 1)".into()); }

    let expected = (hdr.width as usize)
        .checked_mul(hdr.height as usize)
        .and_then(|px| px.checked_mul(4)) // f32
        .ok_or("size overflow")?;

    if payload.len() < expected {
        return Err("payload too short".into());
    }

    let payload = &payload[..expected];
    Ok(RawTile { header: hdr, payload })
}
```

## 3) Upload to `wgpu` as `R32Float`

```rust
// client/src/upload.rs
use wgpu::*;
use crate::parse::RawTile;

pub fn upload_r32float_texture(
    device: &Device,
    queue: &Queue,
    tile: &RawTile,
) -> Texture {
    let size = Extent3d {
        width: tile.header.width,
        height: tile.header.height,
        depth_or_array_layers: 1,
    };
    let tex = device.create_texture(&TextureDescriptor {
        label: Some("raw_r32f"),
        size,
        mip_level_count: 1, // Phase 1
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::R32Float,
        usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
        view_formats: &[],
    });

    let layout = ImageDataLayout {
        offset: 0,
        bytes_per_row: Some(NonZeroU32::new(4 * tile.header.width).unwrap()),
        rows_per_image: Some(NonZeroU32::new(tile.header.height).unwrap()),
    };

    queue.write_texture(
        ImageCopyTexture {
            texture: &tex,
            mip_level: 0,
            origin: Origin3d::ZERO,
            aspect: TextureAspect::All,
        },
        tile.payload,
        layout,
        size,
    );

    tex
}
```

---

# Minimal end-to-end contract

* **Server** guarantees: header exactly 32 bytes, LE; payload tightly packed `float32` H×W.
* **Client** asserts: `dtype_code=f32`, `endianness=little`, `channels=1`, payload length matches `W*H*4`.
* Both sides share **one source of truth** for field order/types and enums.

---
