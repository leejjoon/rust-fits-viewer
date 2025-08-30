# server/routes.py
from fastapi import APIRouter, HTTPException
from fastapi.responses import StreamingResponse
import numpy as np
from pydantic import BaseModel
from .header import RawTileHeader, RAW_HEADER_SIZE, DT_F32, ENDIAN_LITTLE

router = APIRouter()

# Create a global numpy array to simulate a FITS file
IMAGE_DATA = np.arange(1024 * 1024, dtype=np.float32).reshape((1024, 1024))
IMAGE_DATA = (IMAGE_DATA / IMAGE_DATA.max() * 255).astype(np.float32)
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
    # In a real implementation, this would slice a FITS file with downsampling
    # For now, we just slice the global array
    # Note: z is not used yet
    return IMAGE_DATA[y*TILE_SIZE:(y+1)*TILE_SIZE, x*TILE_SIZE:(x+1)*TILE_SIZE]

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
