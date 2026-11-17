//! BC decompression for WebGPU adapters without BC support and exact texel-row flipping.
//! Decoder: pinned MIT bcdec_rs 0.2.0; no Unity/native dependency in the browser graph.
use anyhow::{bail, Context, Result};
use wgpu::TextureFormat as F;

use crate::texture::TextureData;

pub(crate) fn decode(source: &TextureData) -> Result<TextureData> {
    let (format, pixel_bytes) = match source.format {
        F::Bc1RgbaUnorm | F::Bc3RgbaUnorm | F::Bc7RgbaUnorm => (F::Rgba8Unorm, 4),
        F::Bc1RgbaUnormSrgb | F::Bc3RgbaUnormSrgb | F::Bc7RgbaUnormSrgb => (F::Rgba8UnormSrgb, 4),
        F::Bc4RUnorm => (F::R8Unorm, 1),
        F::Bc5RgUnorm => (F::Rg8Unorm, 2),
        F::Bc6hRgbFloat | F::Bc6hRgbUfloat => (F::Rgba16Float, 8),
        other => bail!("texture {:?}: no BC decoder for {other:?}", source.name),
    };
    let block_bytes = source
        .format
        .block_copy_size(None)
        .context("BC block size")? as usize;
    let mut levels = Vec::with_capacity(source.levels.len());
    for (mip, data) in source.levels.iter().enumerate() {
        let width = (source.width >> mip).max(1) as usize;
        let height = (source.height >> mip).max(1) as usize;
        let len = width
            .checked_mul(height)
            .and_then(|n| n.checked_mul(pixel_bytes))
            .context("decoded texture byte count overflow")?;
        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(len)
            .context("allocating decoded texture")?;
        pixels.resize(len, 0);
        let columns = width.div_ceil(4);
        for (index, block) in data.chunks_exact(block_bytes).enumerate() {
            let mut decoded = [0u8; 4 * 4 * 8];
            match source.format {
                F::Bc1RgbaUnorm | F::Bc1RgbaUnormSrgb => bcdec_rs::bc1(block, &mut decoded, 16),
                F::Bc3RgbaUnorm | F::Bc3RgbaUnormSrgb => bcdec_rs::bc3(block, &mut decoded, 16),
                F::Bc4RUnorm => bcdec_rs::bc4(block, &mut decoded, 4, false),
                F::Bc5RgUnorm => bcdec_rs::bc5(block, &mut decoded, 8, false),
                F::Bc7RgbaUnorm | F::Bc7RgbaUnormSrgb => bcdec_rs::bc7(block, &mut decoded, 16),
                F::Bc6hRgbFloat | F::Bc6hRgbUfloat => {
                    let mut rgb = [0u16; 4 * 4 * 3];
                    bcdec_rs::bc6h_half(block, &mut rgb, 12, source.format == F::Bc6hRgbFloat);
                    for (in_pixel, out_pixel) in rgb
                        .as_chunks::<3>()
                        .0
                        .iter()
                        .zip(decoded.as_chunks_mut::<8>().0.iter_mut())
                    {
                        for (half, bytes) in in_pixel
                            .iter()
                            .chain([&0x3c00u16])
                            .zip(out_pixel.as_chunks_mut::<2>().0.iter_mut())
                        {
                            bytes.copy_from_slice(&half.to_le_bytes());
                        }
                    }
                }
                _ => unreachable!("BC format was checked above"),
            }
            let (x, y) = (index % columns * 4, index / columns * 4);
            let count = (width - x).min(4) * pixel_bytes;
            for row in 0..(height - y).min(4) {
                let to = ((y + row) * width + x) * pixel_bytes;
                let from = row * 4 * pixel_bytes;
                pixels[to..to + count].copy_from_slice(&decoded[from..from + count]);
            }
        }
        levels.push(pixels);
    }
    Ok(TextureData {
        name: source.name.clone(),
        width: source.width,
        height: source.height,
        format,
        levels,
    })
}
