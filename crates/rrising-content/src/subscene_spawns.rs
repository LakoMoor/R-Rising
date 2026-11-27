//! Baked subscene spawn requests; these are not live instantiated entities.
use crate::{entity_scenes::SceneFile, type_layouts::TypeLayouts};
use anyhow::{ensure, Result};
const DIRECT: u64 = 0x5993_324a_10db_22a7;
const CHAIN: u64 = 0x6dbf_f1c9_2d85_9a9b;
const INDEX: u64 = 0xdd17_1d94_8ccb_47ea;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpawnKind {
    Direct,
    Chain { prefab_type: u32 },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SubsceneSpawn {
    pub chunk: usize,
    pub row: usize,
    pub guid: i32,
    pub kind: SpawnKind,
    /// Original section-local key; repeated scene uses require runtime chunk remapping.
    pub static_index: Option<(u16, i16)>,
}
pub fn read_spawns(scene: &SceneFile, layouts: &TypeLayouts) -> Result<Vec<SubsceneSpawn>> {
    let hashes = scene.archetypes()?.unique_type_hashes;
    if !hashes.contains(&DIRECT) && !hashes.contains(&CHAIN) {
        return Ok(Vec::new());
    }
    let resolved = layouts.resolve_scene(scene)?;
    let mut out = Vec::new();
    for chunk in &resolved.chunks {
        for row in 0..chunk.header.count as usize {
            for (hash, size) in [(DIRECT, 4), (CHAIN, 8)] {
                let Some(bytes) = resolved.component_bytes(chunk.index, row, hash)? else {
                    continue;
                };
                ensure!(bytes.len() == size, "invalid subscene spawn size");
                let guid = i32::from_le_bytes(bytes[..4].try_into()?);
                let kind = if hash == DIRECT {
                    SpawnKind::Direct
                } else {
                    SpawnKind::Chain {
                        prefab_type: u32::from_le_bytes(bytes[4..8].try_into()?),
                    }
                };
                let static_index = resolved
                    .component_bytes(chunk.index, row, INDEX)?
                    .map(|x| -> Result<_> {
                        ensure!(x.len() == 4, "invalid static index size");
                        Ok((
                            u16::from_le_bytes(x[..2].try_into()?),
                            i16::from_le_bytes(x[2..].try_into()?),
                        ))
                    })
                    .transpose()?;
                out.push(SubsceneSpawn {
                    chunk: chunk.index,
                    row,
                    guid,
                    kind,
                    static_index,
                });
            }
        }
    }
    Ok(out)
}
