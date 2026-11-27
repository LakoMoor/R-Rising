//! Local randomized spawn settings; no random choice or spawning occurs in this reader.
use crate::{entity_scenes::SceneFile, type_layouts::TypeLayouts};
use anyhow::{ensure, Context, Result};
pub const SETTINGS_HASH: u64 = 0x28e8_e86f_84c3_4668;
#[derive(Clone, Debug, PartialEq)]
pub struct ChainWeight {
    pub guid: i32,
    pub weight: f32,
}
#[derive(Clone, Debug, PartialEq)]
pub struct GroupWeight {
    pub chains: Vec<ChainWeight>,
    pub weight: f32,
    pub total_chain_weight: f32,
}
#[derive(Clone, Debug, PartialEq)]
pub struct RandomizedSettings {
    pub groups: Vec<GroupWeight>,
    pub total_group_weight: f32,
}
pub(crate) fn array(blob: &[u8], at: usize, stride: usize) -> Result<(usize, usize)> {
    let descriptor = blob
        .get(at..at.checked_add(8).context("descriptor overflow")?)
        .context("truncated array descriptor")?;
    let relative = i32::from_le_bytes(descriptor[..4].try_into()?);
    let count = i32::from_le_bytes(descriptor[4..].try_into()?);
    ensure!(count >= 0, "negative array length");
    if count == 0 {
        return Ok((0, 0));
    }
    let start =
        usize::try_from(at as i64 + i64::from(relative)).context("negative array offset")?;
    let size = (count as usize)
        .checked_mul(stride)
        .context("array size overflow")?;
    blob.get(start..start.checked_add(size).context("array end overflow")?)
        .context("array outside blob")?;
    Ok((start, count as usize))
}
fn weight(blob: &[u8], at: usize) -> Result<f32> {
    let value = f32::from_le_bytes(
        blob.get(at..at + 4)
            .context("truncated weight")?
            .try_into()?,
    );
    ensure!(value.is_finite(), "nonfinite weight");
    Ok(value)
}
impl RandomizedSettings {
    pub fn read(blob: &[u8]) -> Result<Self> {
        ensure!(blob.len() >= 12, "truncated settings root");
        let (start, count) = array(blob, 0, 16)?;
        let mut groups = Vec::with_capacity(count);
        for i in 0..count {
            let at = start + i * 16;
            let (chain_start, n) = array(blob, at, 8)?;
            let mut chains = Vec::with_capacity(n);
            for j in 0..n {
                let p = chain_start + j * 8;
                chains.push(ChainWeight {
                    guid: i32::from_le_bytes(blob[p..p + 4].try_into()?),
                    weight: weight(blob, p + 4)?,
                });
            }
            groups.push(GroupWeight {
                chains,
                weight: weight(blob, at + 8)?,
                total_chain_weight: weight(blob, at + 12)?,
            });
        }
        Ok(Self {
            groups,
            total_group_weight: weight(blob, 8)?,
        })
    }
}
pub fn read_scene_settings(
    scene: &SceneFile,
    layouts: &TypeLayouts,
) -> Result<Vec<(i32, RandomizedSettings)>> {
    if !scene
        .archetypes()?
        .unique_type_hashes
        .contains(&SETTINGS_HASH)
    {
        return Ok(Vec::new());
    }
    let resolved = layouts.resolve_scene(scene)?;
    let blobs = scene.blob_assets()?;
    let mut out = Vec::new();
    for chunk in &resolved.chunks {
        for row in 0..chunk.header.count as usize {
            let Some(value) = resolved.component_bytes(chunk.index, row, SETTINGS_HASH)? else {
                continue;
            };
            ensure!(value.len() == 8, "invalid settings reference");
            let offset = i32::from_le_bytes(value[..4].try_into()?);
            if offset == -1 {
                continue;
            }
            let guid = resolved
                .component_bytes(chunk.index, row, 0xb181_3197_8820_0ae1)?
                .context("settings missing GUID")?;
            out.push((
                i32::from_le_bytes(guid.try_into()?),
                RandomizedSettings::read(
                    blobs
                        .find_by_offset(offset)
                        .context("settings blob missing")?
                        .data,
                )?,
            ));
        }
    }
    Ok(out)
}
