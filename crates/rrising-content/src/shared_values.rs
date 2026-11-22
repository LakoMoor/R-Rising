//! Typed, little-endian values for the pinned client's unmanaged scene shared components.
//! The records have no entity or blob references according to the recovered type layouts.

use anyhow::{ensure, Result};

pub const TILE_MODEL_LAYER_HASH: u64 = 0x1f83_e8fb_2202_60de;
pub const SCENE_SECTION_HASH: u64 = 0xa466_b7c9_c19e_31f1;
pub const RENDER_FILTER_SETTINGS_HASH: u64 = 0xdb11_cacb_a5e2_db95;

/// The 16 GUID bytes retain the exact serialized representation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SceneSection {
    pub scene_guid: [u8; 16],
    pub section: i32,
}

/// Values use their original integral representation; enum interpretation is not inferred.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderFilterSettings {
    pub layer: i32,
    pub rendering_layer_mask: u32,
    pub motion_mode: i32,
    pub shadow_casting_mode: i32,
    pub receive_shadows: bool,
    pub static_shadow_caster: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KnownSharedValue {
    TileModelLayer(u8),
    SceneSection(SceneSection),
    RenderFilterSettings(RenderFilterSettings),
}

/// Decode one known value. Unknown hashes return `None`; known malformed records return an error.
pub fn decode_known_shared_value(hash: u64, bytes: &[u8]) -> Result<Option<KnownSharedValue>> {
    let read_u32 =
        |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().expect("validated size"));
    Ok(Some(match hash {
        TILE_MODEL_LAYER_HASH => {
            ensure!(bytes.len() == 1, "invalid TileModelLayer size");
            KnownSharedValue::TileModelLayer(bytes[0])
        }
        SCENE_SECTION_HASH => {
            ensure!(bytes.len() == 20, "invalid SceneSection size");
            KnownSharedValue::SceneSection(SceneSection {
                scene_guid: bytes[..16].try_into().expect("validated size"),
                section: read_u32(16) as i32,
            })
        }
        RENDER_FILTER_SETTINGS_HASH => {
            ensure!(bytes.len() == 20, "invalid RenderFilterSettings size");
            ensure!(
                bytes[16] <= 1 && bytes[17] <= 1,
                "invalid RenderFilterSettings boolean"
            );
            KnownSharedValue::RenderFilterSettings(RenderFilterSettings {
                layer: read_u32(0) as i32,
                rendering_layer_mask: read_u32(4),
                motion_mode: read_u32(8) as i32,
                shadow_casting_mode: read_u32(12) as i32,
                receive_shadows: bytes[16] != 0,
                static_shadow_caster: bytes[17] != 0,
            })
        }
        _ => return Ok(None),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_fields_and_rejects_malformed_values() {
        let mut section = [0u8; 20];
        section[..16].copy_from_slice(&[0x5a; 16]);
        section[16..].copy_from_slice(&(-42i32).to_le_bytes());
        assert_eq!(
            decode_known_shared_value(SCENE_SECTION_HASH, &section).unwrap(),
            Some(KnownSharedValue::SceneSection(SceneSection {
                scene_guid: [0x5a; 16],
                section: -42
            }))
        );
        let mut filter = [0u8; 20];
        filter[..4].copy_from_slice(&(-3i32).to_le_bytes());
        filter[4..8].copy_from_slice(&0xaabbccddu32.to_le_bytes());
        filter[8..12].copy_from_slice(&2i32.to_le_bytes());
        filter[12..16].copy_from_slice(&3i32.to_le_bytes());
        filter[16] = 1;
        assert_eq!(
            decode_known_shared_value(RENDER_FILTER_SETTINGS_HASH, &filter).unwrap(),
            Some(KnownSharedValue::RenderFilterSettings(
                RenderFilterSettings {
                    layer: -3,
                    rendering_layer_mask: 0xaabbccdd,
                    motion_mode: 2,
                    shadow_casting_mode: 3,
                    receive_shadows: true,
                    static_shadow_caster: false
                }
            ))
        );
        filter[17] = 2;
        assert!(decode_known_shared_value(RENDER_FILTER_SETTINGS_HASH, &filter).is_err());
        assert!(decode_known_shared_value(TILE_MODEL_LAYER_HASH, &[]).is_err());
        assert_eq!(decode_known_shared_value(0, &[]).unwrap(), None);
    }
}
