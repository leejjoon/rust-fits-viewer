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

const _: () = assert!(RAW_HEADER_SIZE == 32);
