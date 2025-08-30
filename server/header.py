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
