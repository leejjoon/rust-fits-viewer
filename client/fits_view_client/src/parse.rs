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
