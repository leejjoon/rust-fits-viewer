# server/routes.py
from fastapi import APIRouter, HTTPException
from fastapi.responses import StreamingResponse
import numpy as np
from pydantic import BaseModel
from header import RawTileHeader, RAW_HEADER_SIZE, DT_F32, ENDIAN_LITTLE

router = APIRouter()

# Create a global numpy array to simulate a FITS file with circular hole
def create_image_with_hole():
    # Create base gradient image
    image = np.arange(1024 * 1024, dtype=np.float32).reshape((1024, 1024))
    image = (image / image.max() * 255).astype(np.float32)
    
    # Create circular hole at center with radius = 1/3 of image size
    center_y, center_x = 512, 512  # Center of 1024x1024 image
    radius = 1024 // 3  # 1/3 of image size = ~341 pixels
    
    # Create coordinate grids
    y, x = np.ogrid[:1024, :1024]
    
    # Calculate distance from center
    distance = np.sqrt((x - center_x)**2 + (y - center_y)**2)
    
    # Set pixels within radius to 0 (black hole)
    image[distance <= radius] = 0.0
    
    return image

IMAGE_DATA = create_image_with_hole()
TILE_SIZE = 256

class MetaResponse(BaseModel):
    shape: list[int]
    dtype: str
    tile_size: int
    zoom_levels: int
    min_val: float
    max_val: float

@router.get("/meta", response_model=MetaResponse)
def get_meta():
    h, w = IMAGE_DATA.shape
    zoom_levels = int(np.ceil(np.log2(max(h, w) / TILE_SIZE))) + 1
    return MetaResponse(
        shape=[h, w],
        dtype=str(IMAGE_DATA.dtype),
        tile_size=TILE_SIZE,
        zoom_levels=zoom_levels,
        min_val=float(IMAGE_DATA.min()),
        max_val=float(IMAGE_DATA.max()),
    )


def get_tile_numpy(z: int, x: int, y: int) -> np.ndarray:
    # Handle LOD (Level of Detail) by downsampling the image
    # LOD 0 = full resolution, LOD 1 = half resolution, etc.
    
    # Calculate downsampling factor
    downsample_factor = 2 ** z
    
    # Downsample the image for this LOD level
    if z == 0:
        # Full resolution
        source_image = IMAGE_DATA
    else:
        # Downsample by taking every nth pixel
        source_image = IMAGE_DATA[::downsample_factor, ::downsample_factor]
    
    # Validate tile coordinates are within downsampled image bounds
    h, w = source_image.shape
    max_x = (w - 1) // TILE_SIZE  # Maximum valid x coordinate
    max_y = (h - 1) // TILE_SIZE  # Maximum valid y coordinate
    
    if x < 0 or y < 0 or x > max_x or y > max_y:
        raise IndexError(f"Tile ({x},{y}) at LOD {z} is out of bounds. Valid range: (0-{max_x}, 0-{max_y})")
    
    # Calculate slice bounds in the downsampled image
    y_start = y * TILE_SIZE
    y_end = min((y + 1) * TILE_SIZE, h)
    x_start = x * TILE_SIZE  
    x_end = min((x + 1) * TILE_SIZE, w)
    
    tile_data = source_image[y_start:y_end, x_start:x_end]
    
    # Pad tile to full TILE_SIZE if it's at the edge
    if tile_data.shape != (TILE_SIZE, TILE_SIZE):
        padded_tile = np.zeros((TILE_SIZE, TILE_SIZE), dtype=IMAGE_DATA.dtype)
        padded_tile[:tile_data.shape[0], :tile_data.shape[1]] = tile_data
        return padded_tile
    
    return tile_data

@router.get("/tile/{z}/{x}/{y}")
def get_tile(z: int, x: int, y: int, format: str = "raw", dtype: str = "float32"):
    if format == "png":
        raise HTTPException(501, "PNG path not implemented yet")
    elif format == "raw":
        try:
            arr = get_tile_numpy(z, x, y)
        except IndexError:
            raise HTTPException(404, "Tile index out of bounds")

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
