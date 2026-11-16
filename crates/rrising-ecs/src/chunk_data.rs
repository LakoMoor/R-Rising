//! `ArchetypeChunkData`: change versions/shared values are type-major, masks and
//! disabled counts are chunk-major. Source RVAs refer to the pinned GameAssembly.
//! Safe vectors replace one aligned native allocation; unused capacity is zeroed
//! rather than exposing the original allocator's uninitialized bytes.
use crate::{ChunkIndex, K_MAXIMUM_ENTITIES_PER_CHUNK};
use anyhow::{ensure, Result};

#[derive(Clone, Debug)]
pub struct ArchetypeChunkData {
    component_count: usize,
    shared_component_count: usize,
    capacity: usize,
    count: usize,
    chunks: Vec<ChunkIndex>,
    change_versions: Vec<Vec<u32>>,
    entity_counts: Vec<i32>,
    shared_values: Vec<Vec<i32>>,
    enabled: Vec<Vec<[u64; 2]>>,
    disabled: Vec<Vec<i32>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChunkRemoval {
    pub removed: ChunkIndex,
    /// The last chunk, now at the removed index. Caller updates its ListIndex.
    pub moved: Option<ChunkIndex>,
}

impl ArchetypeChunkData {
    /// src: .ctor @ 0x911cf00: component/shared counts survive Dispose.
    pub fn new(component_count: usize, shared_component_count: usize) -> Result<Self> {
        ensure!(component_count > 0, "an archetype must include Entity");
        ensure!(
            shared_component_count < component_count,
            "shared component count exceeds archetype types"
        );
        Ok(Self {
            component_count,
            shared_component_count,
            capacity: 0,
            count: 0,
            chunks: vec![],
            change_versions: vec![vec![]; component_count],
            entity_counts: vec![],
            shared_values: vec![vec![]; shared_component_count],
            enabled: vec![],
            disabled: vec![],
        })
    }
    pub fn count(&self) -> usize {
        self.count
    }
    pub fn capacity(&self) -> usize {
        self.capacity
    }
    pub fn component_count(&self) -> usize {
        self.component_count
    }
    pub fn shared_component_count(&self) -> usize {
        self.shared_component_count
    }
    pub fn chunk_indices(&self) -> &[ChunkIndex] {
        &self.chunks[..self.count]
    }

    /// src: Grow @ 0x911cf20. Exact requested capacity, not a guessed doubling rule.
    pub fn grow(&mut self, capacity: usize) -> Result<()> {
        ensure!(capacity > self.capacity, "chunk capacity must grow");
        ensure!(
            capacity <= i32::MAX as usize,
            "chunk capacity exceeds native int"
        );
        self.buffer_size_for(capacity)?;
        self.chunks.resize(capacity, ChunkIndex::NULL);
        self.entity_counts.resize(capacity, 0);
        for row in &mut self.change_versions {
            row.resize(capacity, 0);
        }
        for row in &mut self.shared_values {
            row.resize(capacity, 0);
        }
        self.enabled
            .resize(capacity, vec![[0; 2]; self.component_count]);
        self.disabled
            .resize(capacity, vec![0; self.component_count]);
        self.capacity = capacity;
        Ok(())
    }
    /// src: get_BufferSize @ 0x911cd50. Padding is 16 even when already aligned.
    pub fn buffer_size_for(&self, capacity: usize) -> Result<usize> {
        let per_chunk = 8usize
            .checked_add(
                self.component_count
                    .checked_mul(4)
                    .ok_or_else(|| anyhow::anyhow!("component count overflow"))?,
            )
            .and_then(|n| n.checked_add(self.shared_component_count.checked_mul(4)?))
            .ok_or_else(|| anyhow::anyhow!("metadata size overflow"))?;
        let prefix = capacity
            .checked_mul(per_chunk)
            .ok_or_else(|| anyhow::anyhow!("metadata size overflow"))?;
        let enabled = capacity
            .checked_mul(self.component_count)
            .and_then(|n| n.checked_mul(20))
            .ok_or_else(|| anyhow::anyhow!("enable data size overflow"))?;
        prefix
            .checked_add(16 - prefix % 16)
            .and_then(|n| n.checked_add(enabled))
            .ok_or_else(|| anyhow::anyhow!("metadata allocation overflow"))
    }
    /// src: Add @ 0x911e0c0. Original gets entity_count from PerChunkArray;
    /// safe host supplies that value explicitly. Enabled data is initialized separately.
    pub fn add(
        &mut self,
        chunk: ChunkIndex,
        entity_count: i32,
        shared_values: &[i32],
        change_version: u32,
    ) -> Result<usize> {
        ensure!(self.count < self.capacity, "grow chunk metadata before Add");
        ensure!(!chunk.is_null(), "cannot add a null chunk");
        ensure!(
            (0..=K_MAXIMUM_ENTITIES_PER_CHUNK).contains(&entity_count),
            "invalid entity count"
        );
        ensure!(
            shared_values.len() == self.shared_component_count,
            "shared component count mismatch"
        );
        let at = self.count;
        self.chunks[at] = chunk;
        self.entity_counts[at] = entity_count;
        for (row, &value) in self.shared_values.iter_mut().zip(shared_values) {
            row[at] = value;
        }
        for row in &mut self.change_versions {
            row[at] = change_version;
        }
        self.count += 1;
        Ok(at)
    }
    fn check_chunk(&self, chunk: usize) -> Result<()> {
        ensure!(
            chunk < self.count,
            "chunk {chunk} outside live count {}",
            self.count
        );
        Ok(())
    }
    fn check_type(&self, component: usize) -> Result<()> {
        ensure!(
            component < self.component_count,
            "component {component} outside archetype"
        );
        Ok(())
    }
    /// src: GetChangeVersionArrayForType @ 0x911deb0.
    pub fn get_change_version_array_for_type(&self, component: usize) -> Result<&[u32]> {
        self.check_type(component)?;
        Ok(&self.change_versions[component][..self.count])
    }
    /// src: GetChangeVersion/SetChangeVersion @ 0x911df80/0x911dfc0.
    pub fn get_change_version(&self, component: usize, chunk: usize) -> Result<u32> {
        self.check_chunk(chunk)?;
        self.check_type(component)?;
        Ok(self.change_versions[component][chunk])
    }
    pub fn set_change_version(
        &mut self,
        component: usize,
        chunk: usize,
        version: u32,
    ) -> Result<()> {
        self.check_chunk(chunk)?;
        self.check_type(component)?;
        self.change_versions[component][chunk] = version;
        Ok(())
    }
    /// src: GetOrderVersion/SetOrderVersion @ 0x911dfa0/0x911e030 use slot 0 (Entity).
    pub fn get_order_version(&self, chunk: usize) -> Result<u32> {
        self.get_change_version(0, chunk)
    }
    pub fn set_order_version(&mut self, chunk: usize, version: u32) -> Result<()> {
        self.set_change_version(0, chunk, version)
    }
    /// src: SetAllChangeVersion @ 0x911dff0 deliberately starts at component 1.
    pub fn set_all_change_version(&mut self, chunk: usize, version: u32) -> Result<()> {
        self.check_chunk(chunk)?;
        for row in self.change_versions.iter_mut().skip(1) {
            row[chunk] = version;
        }
        Ok(())
    }
    /// src: GetChunkEntityCount/SetChunkEntityCount @ 0x911e060/0x911e090.
    pub fn get_chunk_entity_count(&self, chunk: usize) -> Result<i32> {
        self.check_chunk(chunk)?;
        Ok(self.entity_counts[chunk])
    }
    pub fn set_chunk_entity_count(&mut self, chunk: usize, count: i32) -> Result<()> {
        self.check_chunk(chunk)?;
        ensure!(
            (0..=K_MAXIMUM_ENTITIES_PER_CHUNK).contains(&count),
            "invalid entity count"
        );
        self.entity_counts[chunk] = count;
        Ok(())
    }
    /// src: GetSharedComponentValue/SetSharedComponentValue @ 0x911d520/0x911d560.
    pub fn get_shared_component_value(&self, component: usize, chunk: usize) -> Result<i32> {
        self.check_chunk(chunk)?;
        ensure!(
            component < self.shared_component_count,
            "invalid shared type"
        );
        Ok(self.shared_values[component][chunk])
    }
    pub fn set_shared_component_value(
        &mut self,
        component: usize,
        chunk: usize,
        value: i32,
    ) -> Result<()> {
        self.check_chunk(chunk)?;
        ensure!(
            component < self.shared_component_count,
            "invalid shared type"
        );
        self.shared_values[component][chunk] = value;
        Ok(())
    }
    /// src: GetComponentEnabledMaskArrayForChunk @ 0x911d5e0.
    pub fn get_component_enabled_mask_array_for_chunk(&self, chunk: usize) -> Result<&[[u64; 2]]> {
        self.check_chunk(chunk)?;
        Ok(&self.enabled[chunk])
    }
    /// src: GetComponentEnabledMaskArrayForTypeInChunk @ 0x911d780.
    pub fn get_component_enabled_mask_array_for_type_in_chunk(
        &self,
        component: usize,
        chunk: usize,
    ) -> Result<[u64; 2]> {
        self.check_type(component)?;
        self.check_chunk(chunk)?;
        Ok(self.enabled[chunk][component])
    }
    /// src: SetEnabledBitsAndHierarchicalData @ 0x911d920 copies complete rows.
    pub fn set_enabled_bits_and_hierarchical_data(
        &mut self,
        chunk: usize,
        masks: &[[u64; 2]],
        disabled: &[i32],
    ) -> Result<()> {
        self.check_chunk(chunk)?;
        ensure!(
            masks.len() == self.component_count && disabled.len() == self.component_count,
            "enable-data row shape mismatch"
        );
        self.enabled[chunk].copy_from_slice(masks);
        self.disabled[chunk].copy_from_slice(disabled);
        Ok(())
    }
    /// src: InitializeDisabledCountForChunk @ 0x911d830 zeros only hierarchical data.
    pub fn initialize_disabled_count_for_chunk(&mut self, chunk: usize) -> Result<()> {
        self.check_chunk(chunk)?;
        self.disabled[chunk].fill(0);
        Ok(())
    }
    /// src: GetChunkDisabledCountForType/SetChunkDisabledCountForType @ 0x911db10/0x911dcd0.
    pub fn get_chunk_disabled_count_for_type(&self, component: usize, chunk: usize) -> Result<i32> {
        self.check_type(component)?;
        self.check_chunk(chunk)?;
        Ok(self.disabled[chunk][component])
    }
    pub fn set_chunk_disabled_count_for_type(
        &mut self,
        component: usize,
        chunk: usize,
        value: i32,
    ) -> Result<()> {
        self.check_type(component)?;
        self.check_chunk(chunk)?;
        self.disabled[chunk][component] = value;
        Ok(())
    }
    /// src: AdjustChunkDisabledCountForType @ 0x911ddc0: unchecked signed addition.
    pub fn adjust_chunk_disabled_count_for_type(
        &mut self,
        component: usize,
        chunk: usize,
        delta: i32,
    ) -> Result<()> {
        let value = self
            .get_chunk_disabled_count_for_type(component, chunk)?
            .wrapping_add(delta);
        self.set_chunk_disabled_count_for_type(component, chunk, value)
    }
    /// src: RemoveAtSwapBack @ 0x911e2f0 moves every live metadata column/row.
    pub fn remove_at_swap_back(&mut self, at: usize) -> Result<ChunkRemoval> {
        self.check_chunk(at)?;
        let last = self.count - 1;
        let removed = self.chunks[at];
        let moved = if at != last {
            self.chunks[at] = self.chunks[last];
            self.entity_counts[at] = self.entity_counts[last];
            for row in &mut self.change_versions {
                row[at] = row[last];
            }
            for row in &mut self.shared_values {
                row[at] = row[last];
            }
            let (before, tail) = self.enabled.split_at_mut(last);
            before[at].clone_from(&tail[0]);
            let (before, tail) = self.disabled.split_at_mut(last);
            before[at].clone_from(&tail[0]);
            Some(self.chunks[at])
        } else {
            None
        };
        self.count = last;
        Ok(ChunkRemoval { removed, moved })
    }
    /// src: Dispose @ 0x911e550 clears allocation/capacity/count, not type counts.
    pub fn dispose(&mut self) {
        self.chunks.clear();
        self.entity_counts.clear();
        for row in &mut self.change_versions {
            row.clear();
        }
        for row in &mut self.shared_values {
            row.clear();
        }
        self.enabled.clear();
        self.disabled.clear();
        self.capacity = 0;
        self.count = 0;
    }
}
