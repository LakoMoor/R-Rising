//! Uploading a stored `Texture2D` with hardware BC or a portable CPU decoder.
//!
//! The game's art is compressed (BC1/BC3/BC4/BC5/BC6h/BC7 and the odd uncompressed RGBA). wgpu can
//! hold those block formats directly on Metal and D3D11/Vulkan, so the port uploads the file's own
//! blocks when the format and orientation allow it. Otherwise it decodes to uncompressed texels. Crunch (Unity's `DXT1Crunched`/`DXT5Crunched`)
//! is unpacked by `unity_formats::texture::UnityTexture::levels`, which hands back the DXT blocks
//! the crunch stream decodes to — still the shipped pixels, just Huffman-decoded, which is what the
//! original's own loader does at load time. BC6H/BC7 decode before the texel-row flip; BC1/3/4/5
//! use the same fallback without hardware BC support. CPU and GPU BC1..5 palette interpolation
//! can differ; measured bounds are recorded in docs/COMPATIBILITY.md.
//!
//! One orientation fact matters: `UnityTexture::levels` returns each mip **bottom row first**, while
//! wgpu textures start at the top row. The upload flips each level's rows, which is the only place
//! orientation is touched; the shader's uv stays as the file stores it.

use anyhow::{anyhow, Result};
use unity_formats::texture::{TextureFormat, UnityTexture};

use crate::texture::{GpuTexture, TextureData};

/// The wgpu format that holds Unity's `format` with the file's own sRGB flag.
pub fn map_format(format: TextureFormat, srgb: bool) -> Result<wgpu::TextureFormat> {
    use TextureFormat as U;
    let f = match format {
        U::Dxt1 => {
            if srgb {
                wgpu::TextureFormat::Bc1RgbaUnormSrgb
            } else {
                wgpu::TextureFormat::Bc1RgbaUnorm
            }
        }
        U::Dxt5 => {
            if srgb {
                wgpu::TextureFormat::Bc3RgbaUnormSrgb
            } else {
                wgpu::TextureFormat::Bc3RgbaUnorm
            }
        }
        U::Bc7 => {
            if srgb {
                wgpu::TextureFormat::Bc7RgbaUnormSrgb
            } else {
                wgpu::TextureFormat::Bc7RgbaUnorm
            }
        }
        U::Bc5 => wgpu::TextureFormat::Bc5RgUnorm,
        U::Bc4 => wgpu::TextureFormat::Bc4RUnorm,
        // unity-formats' TextureFormat::Bc6h is unsigned half-float RGB.
        U::Bc6h => wgpu::TextureFormat::Bc6hRgbUfloat,
        U::Rgba32 => {
            if srgb {
                wgpu::TextureFormat::Rgba8UnormSrgb
            } else {
                wgpu::TextureFormat::Rgba8Unorm
            }
        }
        U::Bgra32 => {
            if srgb {
                wgpu::TextureFormat::Bgra8UnormSrgb
            } else {
                wgpu::TextureFormat::Bgra8Unorm
            }
        }
        U::RgbaFloat => wgpu::TextureFormat::Rgba32Float,
        U::RgbaHalf => wgpu::TextureFormat::Rgba16Float,
        U::RFloat => wgpu::TextureFormat::R32Float,
        U::RgFloat => wgpu::TextureFormat::Rg32Float,
        U::R16 => wgpu::TextureFormat::R16Unorm,
        U::Alpha8 => wgpu::TextureFormat::R8Unorm,
        other => {
            return Err(anyhow!(
                "no wgpu format chosen for Unity texture format {other:?} (unported)"
            ));
        }
    };
    Ok(f)
}

/// Upload a texture with its whole mip chain.
pub fn upload(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    tex: &UnityTexture,
    features: wgpu::Features,
) -> Result<GpuTexture> {
    // `levels` gives the format the *data* is in: a crunched texture comes back as the DXT blocks it
    // unpacks to, so the format is taken from here and not re-derived from `tex.format`.
    let (format, levels) = tex
        .levels()
        .map_err(|e| anyhow!("texture {:?}: {e}", tex.name))?;
    let data = TextureData {
        name: tex.name.clone(),
        width: tex.width,
        height: tex.height,
        format: map_format(format, tex.srgb)?,
        levels: levels.iter().map(|x| x.to_vec()).collect(),
    };
    data.upload(device, queue, features)
}

pub fn export(tex: &UnityTexture) -> Result<TextureData> {
    let (format, levels) = tex
        .levels()
        .map_err(|e| anyhow!("texture {:?}: {e}", tex.name))?;
    Ok(TextureData {
        name: tex.name.clone(),
        width: tex.width,
        height: tex.height,
        format: map_format(format, tex.srgb)?,
        levels: levels.iter().map(|x| x.to_vec()).collect(),
    })
}
