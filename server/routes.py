# server/routes.py
from fastapi import APIRouter, HTTPException
from fastapi.responses import StreamingResponse
import numpy as np
from .header import RawTileHeader, RAW_HEADER_SIZE, DT_F32, ENDIAN_LITTLE

router = APIRouter()

def get_tile_numpy(z: int, x: int, y: int) -> np.ndarray:
    # Placeholder: return a dummy 256x256 array
    # In a real implementation, this would slice a FITS file
    return np.random.rand(256, 256).astype(np.float32)

@router.get("/tile/{z}/{x}/{y}")
def get_tile(z: int, x: int, y: int, format: str = "raw", dtype: str = "float32"):
    if format == "png":
        raise HTTPException(501, "PNG path not implemented yet")
    elif format == "raw":
        arr = get_tile_numpy(z, x, y)
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
