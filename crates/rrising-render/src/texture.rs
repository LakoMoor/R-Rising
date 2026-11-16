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
            // Preserve the native uploader's orientation, including its known BC-block limitation.
            let mut flipped = Vec::with_capacity(data.len());
            for row in (0..rows as usize).rev() {
                let start = row * row_bytes as usize;
                flipped.extend_from_slice(&data[start..start + row_bytes as usize]);
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
