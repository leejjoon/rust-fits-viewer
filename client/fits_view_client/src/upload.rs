// client/src/upload.rs
use wgpu::*;
use crate::parse::RawTile;
use std::num::NonZeroU32;

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
