//! Bridge serialized EnabledBits/Chunks into the ported ArchetypeChunkData.
//! This restores chunk metadata, not component relocation or a simulated world.
use crate::entity_scenes::SceneFile;
use anyhow::{ensure, Result};
use rrising_ecs::{ArchetypeChunkData, ChunkIndex};

pub fn load_chunk_metadata(
    scene: &SceneFile,
    initial_change_version: u32,
) -> Result<Vec<ArchetypeChunkData>> {
    let archetypes = scene.archetypes()?;
    let chunks = scene.chunks()?;
    let bits = scene.enabled_bits()?;
    ensure!(
        bits.component_counts.len() == archetypes.archetypes.len(),
        "scene lacks enabled-bit metadata"
    );
    let mut metadata = Vec::with_capacity(archetypes.archetypes.len());
    for (index, archetype) in archetypes.archetypes.iter().enumerate() {
        // Shared values remain in the separate serialized remap stream; this bridge
        // deliberately does not invent their runtime registry indices.
        let mut data = ArchetypeChunkData::new(archetype.types.len() + 1, 0)?;
        if bits.chunk_counts[index] > 0 {
            data.grow(bits.chunk_counts[index])?;
        }
        metadata.push(data);
    }
    let mut starts = Vec::with_capacity(metadata.len());
    let mut offset = 0;
    for i in 0..metadata.len() {
        starts.push(offset);
        offset += bits.chunk_counts[i] * bits.component_counts[i];
    }
    let mut entities = vec![0i64; metadata.len()];
    for chunk in chunks {
        let archetype = usize::try_from(chunk.header.archetype_index)?;
        let data = metadata
            .get_mut(archetype)
            .ok_or_else(|| anyhow::anyhow!("chunk references unknown archetype"))?;
        let handle = i32::try_from(chunk.index + 1)?;
        let ordinal = data.add(
            ChunkIndex::new(handle),
            chunk.header.count,
            &[],
            initial_change_version,
        )?;
        entities[archetype] += i64::from(chunk.header.count);
        let start = starts[archetype] + ordinal * data.component_count();
        let end = start + data.component_count();
        let masks: Vec<_> = bits.masks[start..end]
            .iter()
            .map(|mask| [mask.low, mask.high])
            .collect();
        data.set_enabled_bits_and_hierarchical_data(
            ordinal,
            &masks,
            &bits.hierarchical[start..end],
        )?;
    }
    for (index, archetype) in archetypes.archetypes.iter().enumerate() {
        ensure!(
            entities[index] == i64::from(archetype.entity_count),
            "archetype {index} entity count differs from chunk total"
        );
    }
    Ok(metadata)
}
