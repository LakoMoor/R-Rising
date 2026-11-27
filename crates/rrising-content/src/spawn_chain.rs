//! Structural spawn-chain definitions; transition execution and scheduling are absent.
use crate::{entity_scenes::SceneFile, randomized_settings::array, type_layouts::TypeLayouts};
use anyhow::{ensure, Context, Result};
const CONSTANTS: u64 = 0x7168_4f1d_5183_e7ff;
#[derive(Clone, Debug, PartialEq)]
pub struct ChainTransition {
    pub id: [u8; 32],
    pub target: i32,
    pub importance: i32,
    pub end_of_chain: bool,
}
#[derive(Clone, Debug, PartialEq)]
pub struct AutoTransitionConfig {
    pub transition_id: [u8; 32],
    pub transition_index: i32,
    pub attempt_cooldown: f32,
    pub duration: f32,
}
#[derive(Clone, Debug, PartialEq)]
pub struct ChainElement {
    pub index: i32,
    pub prefab_guid: i32,
    pub use_auto_transition: bool,
    pub auto_transition: AutoTransitionConfig,
    pub transitions: Vec<ChainTransition>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct ChainDefinition {
    pub main_index: i32,
    pub elements: Vec<ChainElement>,
}
impl ChainDefinition {
    pub fn main_element(&self) -> Option<&ChainElement> {
        self.elements.get(usize::try_from(self.main_index).ok()?)
    }
    pub fn read(blob: &[u8]) -> Result<Self> {
        ensure!(blob.len() >= 12, "truncated chain root");
        let (start, count) = array(blob, 0, 96)?;
        let mut elements = Vec::with_capacity(count);
        for i in 0..count {
            let at = start + i * 96;
            let (p, n) = array(blob, at, 44)?;
            let mut transitions = Vec::with_capacity(n);
            for j in 0..n {
                let entry = &blob[p + j * 44..p + (j + 1) * 44];
                ensure!(entry[40] <= 1, "invalid transition boolean");
                transitions.push(ChainTransition {
                    id: entry[..32].try_into()?,
                    target: i32::from_le_bytes(entry[32..36].try_into()?),
                    importance: i32::from_le_bytes(entry[36..40].try_into()?),
                    end_of_chain: entry[40] != 0,
                });
            }
            ensure!(blob[at + 48] <= 1, "invalid auto transition boolean");
            elements.push(ChainElement {
                index: i32::from_le_bytes(blob[at + 40..at + 44].try_into()?),
                prefab_guid: i32::from_le_bytes(blob[at + 44..at + 48].try_into()?),
                use_auto_transition: blob[at + 48] != 0,
                auto_transition: AutoTransitionConfig {
                    transition_id: blob[at + 52..at + 84].try_into()?,
                    transition_index: i32::from_le_bytes(blob[at + 84..at + 88].try_into()?),
                    attempt_cooldown: f32::from_le_bytes(blob[at + 88..at + 92].try_into()?),
                    duration: f32::from_le_bytes(blob[at + 92..at + 96].try_into()?),
                },
                transitions,
            });
        }
        Ok(Self {
            main_index: i32::from_le_bytes(blob[8..12].try_into()?),
            elements,
        })
    }
}
pub fn read_scene_chains(
    scene: &SceneFile,
    layouts: &TypeLayouts,
) -> Result<Vec<(i32, ChainDefinition)>> {
    if !scene.archetypes()?.unique_type_hashes.contains(&CONSTANTS) {
        return Ok(Vec::new());
    }
    let resolved = layouts.resolve_scene(scene)?;
    let blobs = scene.blob_assets()?;
    let mut out = Vec::new();
    for chunk in &resolved.chunks {
        for row in 0..chunk.header.count as usize {
            let Some(value) = resolved.component_bytes(chunk.index, row, CONSTANTS)? else {
                continue;
            };
            ensure!(value.len() == 8, "invalid chain reference");
            let offset = i32::from_le_bytes(value[..4].try_into()?);
            if offset == -1 {
                continue;
            }
            let guid = resolved
                .component_bytes(chunk.index, row, 0xb181_3197_8820_0ae1)?
                .context("chain missing GUID")?;
            out.push((
                i32::from_le_bytes(guid.try_into()?),
                ChainDefinition::read(
                    blobs
                        .find_by_offset(offset)
                        .context("chain blob missing")?
                        .data,
                )?,
            ));
        }
    }
    Ok(out)
}
