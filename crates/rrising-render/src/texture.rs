//! Platform-neutral stored texture upload. Original Unity adapters are optional.
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};

pub struct GpuTexture {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub view: wgpu::TextureView,
}

#[derive(Serialize, Deserialize)]
pub struct TextureData {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub format: wgpu::TextureFormat,
    /// Original bottom-first mip data; compressed formats retain their blocks.
    pub levels: Vec<Vec<u8>>,
}

impl TextureData {
    /// A 1x1 sRGB texture of one flat colour: the neutral stand-in for a base map that is missing
    /// or could not be uploaded, so the gap reads as grey rather than as an unlit white surface.
    pub fn flat(name: &str, rgba: [u8; 4]) -> Self {
        Self {
            name: name.to_owned(),
            width: 1,
            height: 1,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            levels: vec![rgba.to_vec()],
        }
    }

    pub fn upload(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        features: wgpu::Features,
    ) -> Result<GpuTexture> {
        ensure!(
            self.width > 0 && self.height > 0,
            "texture {:?}: zero dimension",
            self.name
        );
        ensure!(
            self.width <= device.limits().max_texture_dimension_2d
                && self.height <= device.limits().max_texture_dimension_2d,
            "texture {:?}: exceeds device dimensions",
            self.name
        );
        ensure!(
            !self.levels.is_empty()
                && self.levels.len() as u32 <= self.width.max(self.height).ilog2() + 1,
            "texture {:?}: invalid mip count",
            self.name
        );
        ensure!(
            features.contains(self.format.required_features()),
            "texture {:?}: adapter lacks {:?}",
            self.name,
            self.format.required_features()
        );
        let (bw, bh) = self.format.block_dimensions();
        let block = self
            .format
            .block_copy_size(None)
            .ok_or_else(|| anyhow::anyhow!("unsupported texture format {:?}", self.format))?;
        // Refuse depth, integer and nonfilterable textures before creating bind groups.
        ensure!(
            matches!(
                self.format,
                wgpu::TextureFormat::Bc1RgbaUnorm
                    | wgpu::TextureFormat::Bc1RgbaUnormSrgb
                    | wgpu::TextureFormat::Bc3RgbaUnorm
                    | wgpu::TextureFormat::Bc3RgbaUnormSrgb
                    | wgpu::TextureFormat::Bc4RUnorm
                    | wgpu::TextureFormat::Bc5RgUnorm
                    | wgpu::TextureFormat::Bc6hRgbFloat
                    | wgpu::TextureFormat::Bc7RgbaUnorm
                    | wgpu::TextureFormat::Bc7RgbaUnormSrgb
                    | wgpu::TextureFormat::Rgba8Unorm
                    | wgpu::TextureFormat::Rgba8UnormSrgb
                    | wgpu::TextureFormat::Bgra8Unorm
                    | wgpu::TextureFormat::Bgra8UnormSrgb
                    | wgpu::TextureFormat::R8Unorm
                    | wgpu::TextureFormat::Rgba16Float
            ),
            "texture {:?}: format {:?} is not supported by the filtered material path",
            self.name,
            self.format
        );
        for (mip, data) in self.levels.iter().enumerate() {
            let w = (self.width >> mip).max(1).div_ceil(bw);
            let h = (self.height >> mip).max(1).div_ceil(bh);
            ensure!(
                data.len() == w as usize * h as usize * block as usize,
                "texture {:?} mip {mip}: invalid byte count",
                self.name
            );
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(&self.name),
            size: wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: self.levels.len() as u32,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (mip, data) in self.levels.iter().enumerate() {
            let pw = (self.width >> mip).max(1).next_multiple_of(bw);
            let ph = (self.height >> mip).max(1).next_multiple_of(bh);
            let row_bytes = pw / bw * block;
            let rows = ph / bh;
            // Flip block rows, then texel rows inside each block (BC1/3/4/5; BC6H/BC7 cannot
            // be flipped without re-encoding and keep the block-row-only flip).
            let mut flipped = Vec::with_capacity(data.len());
            for row in (0..rows as usize).rev() {
                let start = row * row_bytes as usize;
                flipped.extend_from_slice(&data[start..start + row_bytes as usize]);
            }
            let valid_rows = ((self.height >> mip).max(1) as usize).min(bh as usize);
            if bh == 4 {
                for b in flipped.chunks_exact_mut(block as usize) {
                    flip_block_rows(self.format, b, valid_rows);
                }
            }
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: mip as u32,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &flipped,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row_bytes),
                    rows_per_image: Some(rows),
                },
                wgpu::Extent3d {
                    width: pw,
                    height: ph,
                    depth_or_array_layers: 1,
                },
            );
        }
        Ok(GpuTexture {
            name: self.name.clone(),
            width: self.width,
            height: self.height,
            view: texture.create_view(&Default::default()),
        })
    }
}

/// Reverse the first `rows` texel rows of one BC1/BC3/BC4/BC5 block in place.
/// Other formats are left untouched (their blocks are not row-separable).
fn flip_block_rows(format: wgpu::TextureFormat, b: &mut [u8], rows: usize) {
    use wgpu::TextureFormat as F;
    match format {
        F::Bc1RgbaUnorm | F::Bc1RgbaUnormSrgb => flip_bc1_color(b, rows),
        F::Bc3RgbaUnorm | F::Bc3RgbaUnormSrgb => {
            flip_bc4(&mut b[..8], rows);
            flip_bc1_color(&mut b[8..], rows);
        }
        F::Bc4RUnorm => flip_bc4(b, rows),
        F::Bc5RgUnorm => {
            flip_bc4(&mut b[..8], rows);
            flip_bc4(&mut b[8..], rows);
        }
        _ => {}
    }
}

/// BC1 color block: bytes 4..8 hold one index byte per texel row.
fn flip_bc1_color(b: &mut [u8], rows: usize) {
    b[4..4 + rows].reverse();
}

/// BC4 block: 48 index bits (little endian) after two endpoints, 12 bits per texel row.
fn flip_bc4(b: &mut [u8], rows: usize) {
    let mut bits = 0u64;
    for (i, v) in b[2..8].iter().enumerate() {
        bits |= (*v as u64) << (8 * i);
    }
    let mut out = bits;
    for r in 0..rows {
        let src = (bits >> (12 * (rows - 1 - r))) & 0xfff;
        out = (out & !(0xfffu64 << (12 * r))) | (src << (12 * r));
    }
    for (i, v) in b[2..8].iter_mut().enumerate() {
        *v = (out >> (8 * i)) as u8;
    }
}

#[cfg(test)]
mod flip_tests {
    use super::*;

    #[test]
    fn bc1_rows_reverse() {
        let mut b = [0, 0, 0, 0, 1, 2, 3, 4];
        flip_block_rows(wgpu::TextureFormat::Bc1RgbaUnorm, &mut b, 4);
        assert_eq!(b, [0, 0, 0, 0, 4, 3, 2, 1]);
        let mut b = [0, 0, 0, 0, 1, 2, 3, 4];
        flip_block_rows(wgpu::TextureFormat::Bc1RgbaUnorm, &mut b, 1);
        assert_eq!(b, [0, 0, 0, 0, 1, 2, 3, 4]);
        let mut b = [0, 0, 0, 0, 1, 2, 3, 4];
        flip_block_rows(wgpu::TextureFormat::Bc1RgbaUnorm, &mut b, 2);
        assert_eq!(b, [0, 0, 0, 0, 2, 1, 3, 4]);
    }

    #[test]
    fn bc4_rows_reverse() {
        // rows 0..3 = 0x111, 0x222, 0x333, 0x444 (12 bits each)
        let bits: u64 = 0x111 | (0x222 << 12) | (0x333 << 24) | (0x444 << 36);
        let mut b = [9u8, 8, 0, 0, 0, 0, 0, 0];
        for i in 0..6 {
            b[2 + i] = (bits >> (8 * i)) as u8;
        }
        flip_block_rows(wgpu::TextureFormat::Bc4RUnorm, &mut b, 4);
        let mut out = 0u64;
        for i in 0..6 {
            out |= (b[2 + i] as u64) << (8 * i);
        }
        assert_eq!(out, 0x444 | (0x333 << 12) | (0x222 << 24) | (0x111 << 36));
        assert_eq!(&b[..2], &[9, 8]);
    }
}
