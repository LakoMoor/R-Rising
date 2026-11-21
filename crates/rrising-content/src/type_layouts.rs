//! Read-only bridge from an offline type-layout report to original scene component bytes.
//!
//! Reports come from `tools/probe_type_registry.py` (pinned binary/metadata identities,
//! scene-hash matching, native field offsets, and inspected TypeHash/BuildComponentType).
//! This is not a runtime TypeInfo table: serialization/refcount/write-group behavior is
//! unknown. Only archetypes whose hashes all have recovered storage inputs are attempted;
//! the chunk image must then pass Entity/buffer-boundary checks. Unknown TypeIndex
//! flags remain a limitation: passing these checks is not a runtime TypeInfo oracle. Serialized pointers
//! are checked as bytes and are never converted into host addresses.
use std::collections::{BTreeMap, BTreeSet};

use anyhow::{anyhow, ensure, Context, Result};
use rrising_ecs::{
    archetype::create_archetype_data_for_buffer, ArchetypeData, Entity, SpecialTypes, TypeCategory,
    TypeIndex, TypeManager, TypeSpec,
};
use serde_json::Value;

use crate::entity_scenes::{SceneArchetypes, SceneChunk, SceneEnabledBits, SceneFile};

/// Chunk bytes a managed `IComponentData` class occupies per entity (a managed-store index).
pub const MANAGED_COMPONENT_SLOT_BYTES: i32 = 4;
pub const ENTITY_HASH: u64 = 0x6f02_124d_960c_5b29;
pub const TRANSLATION_HASH: u64 = 0xa42d_e7cc_0763_e5be;
pub const ROTATION_HASH: u64 = 0xb1ac_0344_99f4_9e3e;
pub const LOCAL_TO_WORLD_HASH: u64 = 0x9749_0261_b3c2_de08;
pub const LOCAL_TRANSFORM_HASH: u64 = 0x5c5d_60f2_17e4_1458;

#[derive(Clone, Debug)]
pub struct RecoveredType {
    pub stable_hash: u64,
    pub name: String,
    pub spec: TypeSpec,
    pub entity_offsets: Vec<i32>,
    pub portable_value: bool,
}

/// Storage inputs recovered offline; unknown hashes remain unknown.
#[derive(Clone, Debug)]
pub struct TypeLayouts {
    pub binary_sha256: String,
    pub metadata_sha256: String,
    by_hash: BTreeMap<u64, RecoveredType>,
}

fn integer(v: &Value, key: &str) -> Result<i32> {
    let n = v
        .get(key)
        .and_then(Value::as_i64)
        .with_context(|| format!("missing integer {key}"))?;
    i32::try_from(n).with_context(|| format!("integer {key} overflows i32"))
}
fn string<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v.get(key)
        .and_then(Value::as_str)
        .with_context(|| format!("missing string {key}"))
}
fn boolean(v: &Value, key: &str) -> Result<bool> {
    v.get(key)
        .and_then(Value::as_bool)
        .with_context(|| format!("missing boolean {key}"))
}
fn hex(v: &Value, key: &str) -> Result<u64> {
    u64::from_str_radix(string(v, key)?.trim_start_matches("0x"), 16)
        .with_context(|| format!("invalid hexadecimal {key}"))
}

impl TypeLayouts {
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        Self::from_pinned_json(bytes, false)
    }

    pub fn from_server_json(bytes: &[u8]) -> Result<Self> {
        Self::from_pinned_json(bytes, true)
    }

    fn from_pinned_json(bytes: &[u8], server: bool) -> Result<Self> {
        let report: Value = serde_json::from_slice(bytes).context("type layout report JSON")?;
        ensure!(
            integer(&report, "schema_version")? == 1,
            "unsupported type layout schema"
        );
        ensure!(
            string(&report, "evidence_kind")? == "offline-derived-and-scene-hash-matched",
            "unrecognized layout provenance"
        );
        let binary_sha256 = string(&report, "binary_sha256")?.to_owned();
        let metadata_sha256 = string(&report, "metadata_sha256")?.to_owned();
        let (expected_binary, expected_metadata) = if server {
            (
                "41ba4eb27c53dd9dc1a3d3337a2c8550b8386421361792ee6ac159ef00ff0ac7",
                "80ec821655b86d5a891c0268e3b10891de810535a099cbed8a45bfc647141bb5",
            )
        } else {
            (
                "a23f36d5b5b0e393fb77dc0680a9600016173fea6c0590bf2634f732862b0a56",
                "b8f9f76ffb72a0c77079082aac8eb78da839094e9d090e8461b49230e8831271",
            )
        };
        ensure!(
            binary_sha256 == expected_binary && metadata_sha256 == expected_metadata,
            "layout report is for a different original build"
        );
        let mut by_hash = BTreeMap::new();
        let rows = report
            .get("matches")
            .and_then(Value::as_array)
            .context("missing layout matches")?;
        let server_tags = match report.get("server_tags") {
            Some(tags) if server => tags.as_array().context("invalid server tags")?.as_slice(),
            _ => &[],
        };
        let server_values = match report.get("server_values") {
            Some(values) if server => values
                .as_array()
                .context("invalid server values")?
                .as_slice(),
            _ => &[],
        };
        ensure!(
            rows.len() + server_tags.len() + server_values.len()
                < rrising_ecs::K_MAX_TYPE_COUNT - 1,
            "too many recovered types"
        );
        for (row, supplemental_tag) in rows
            .iter()
            .map(|row| (row, false))
            .chain(server_tags.iter().map(|row| (row, true)))
            .chain(server_values.iter().map(|row| (row, false)))
        {
            let name = string(row, "name")?;
            let stable_hash = hex(row, "stable_hash")?;
            let ecs = row.get("ecs").context("missing recovered storage shape")?;
            let managed = boolean(ecs, "managed")?;
            let category = match string(ecs, "category")? {
                "component" => TypeCategory::ComponentData,
                "buffer" => TypeCategory::BufferData,
                "shared" => TypeCategory::ISharedComponentData,
                "entity" => TypeCategory::EntityData,
                other => return Err(anyhow!("unsupported category {other:?} for {name}")),
            };
            // BuildComponentType @ RVA 0x905a560 (managed-class branch at 0x905afa7) stores
            // SizeInChunk = ElementSize = 4 (the managed-store index), alignment 0 and
            // BufferCapacity -1 for a managed IComponentData class; no other managed per-entity
            // shape is recovered.
            ensure!(
                !managed
                    || category == TypeCategory::ISharedComponentData
                    || (category == TypeCategory::ComponentData
                        && integer(ecs, "size_in_chunk")? == MANAGED_COMPONENT_SLOT_BYTES
                        && integer(ecs, "element_size")? == MANAGED_COMPONENT_SLOT_BYTES),
                "per-entity managed storage shape is unrecovered: {name}"
            );
            let size = integer(ecs, "size_in_chunk")?;
            if supplemental_tag {
                ensure!(
                    category == TypeCategory::ComponentData
                        && size == 0
                        && !managed
                        && !boolean(ecs, "cleanup")?,
                    "invalid supplemental server tag {name}"
                );
            }
            let element_size = integer(ecs, "element_size")?;
            let capacity = if category == TypeCategory::BufferData {
                integer(ecs, "buffer_capacity")?
            } else {
                -1
            };
            let maximum = integer(ecs, "maximum_chunk_capacity")?;
            let alignment = integer(ecs, "alignment_in_bytes")?;
            let type_size = integer(row, "managed_value_size")?;
            ensure!(
                (0..=rrising_ecs::K_BUFFER_SIZE as i32).contains(&size)
                    && (0..=rrising_ecs::K_BUFFER_SIZE as i32).contains(&element_size)
                    && (0..=rrising_ecs::K_BUFFER_SIZE as i32).contains(&type_size),
                "invalid component size for {name}"
            );
            ensure!(
                (1..=128).contains(&maximum),
                "unsupported chunk maximum for {name}"
            );
            ensure!(
                alignment == 0
                    || alignment > 0 && alignment <= 16 && (alignment as u32).is_power_of_two(),
                "invalid alignment for {name}"
            );
            ensure!(
                boolean(ecs, "zero_sized")? == (size == 0),
                "zero-sized flag disagrees with storage for {name}"
            );
            if category == TypeCategory::BufferData {
                ensure!(
                    capacity >= 0 && element_size > 0,
                    "invalid buffer shape for {name}"
                );
                ensure!(
                    capacity
                        .checked_mul(element_size)
                        .and_then(|n| n.checked_add(16))
                        == Some(size),
                    "buffer inline capacity/stride mismatch for {name}"
                );
            }
            if category == TypeCategory::ISharedComponentData {
                ensure!(
                    size == 0 && element_size == 0,
                    "shared component occupies chunk bytes: {name}"
                );
            }
            let entity_offsets = ecs
                .get("entity_offsets")
                .and_then(Value::as_array)
                .context("missing entity reference offsets")?
                .iter()
                .map(|n| {
                    let n = n.as_i64().context("entity offset is not integer")?;
                    let n = i32::try_from(n).context("entity offset overflow")?;
                    ensure!(
                        n >= 0 && n.checked_add(8).is_some_and(|end| end <= type_size),
                        "entity reference outside {name}"
                    );
                    Ok(n)
                })
                .collect::<Result<Vec<_>>>()?;
            let mut flags = if managed {
                TypeIndex::K_MANAGED_MASK
            } else {
                0
            };
            if size == 0 {
                flags |= TypeIndex::K_ZERO_SIZED_MASK;
            }
            if category == TypeCategory::BufferData {
                flags |= TypeIndex::K_BUFFER_MASK;
            }
            if category == TypeCategory::ISharedComponentData {
                flags |= TypeIndex::K_SHARED_MASK;
            }
            if boolean(ecs, "enableable")? {
                flags |= TypeIndex::K_ENABLEABLE_MASK;
            }
            // BuildComponentType sets bit 23 when any inherited interface is an IEquatable<>
            // instance (lambda RVA 0x9065a80). Every flag bit participates in memory ordering,
            // so omitting it mis-orders buffers (ProfessorCoilPairElement sorts last).
            if boolean(ecs, "iequatable")? {
                flags |= TypeIndex::K_IEQUATABLE_MASK;
            }
            if boolean(ecs, "cleanup")? {
                flags |= TypeIndex::K_CLEANUP_MASK;
            }
            // This build sets the bit when entity references are absent.
            if entity_offsets.is_empty() {
                flags |= TypeIndex::K_HAS_ENTITY_REFERENCES_MASK;
            }
            let mut spec = TypeSpec::component_data(name, size, alignment);
            spec.category = category;
            spec.size_in_chunk = size;
            spec.element_size = element_size;
            spec.type_size = type_size;
            spec.buffer_capacity = capacity;
            spec.maximum_chunk_capacity = maximum;
            spec.flags = flags;
            spec.memory_ordering = hex(ecs, "memory_ordering")?;
            spec.stable_type_hash = stable_hash;
            spec.entity_offset_count = i32::try_from(entity_offsets.len())?;
            let portable_value = ecs
                .get("portable_value")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            ensure!(
                by_hash
                    .insert(
                        stable_hash,
                        RecoveredType {
                            stable_hash,
                            name: name.to_owned(),
                            spec,
                            entity_offsets,
                            portable_value,
                        }
                    )
                    .is_none(),
                "duplicate recovered stable hash {stable_hash:016x}"
            );
        }
        let entity = by_hash
            .get(&ENTITY_HASH)
            .context("layout report omits Entity")?;
        ensure!(
            entity.name == "Unity.Entities.Entity"
                && entity.spec.size_in_chunk == 8
                && entity.spec.memory_ordering == 0,
            "Entity special registration mismatch"
        );
        Ok(Self {
            binary_sha256,
            metadata_sha256,
            by_hash,
        })
    }

    pub fn len(&self) -> usize {
        self.by_hash.len()
    }
    pub fn is_empty(&self) -> bool {
        self.by_hash.is_empty()
    }
    pub fn get(&self, stable_hash: u64) -> Option<&RecoveredType> {
        self.by_hash.get(&stable_hash)
    }

    pub(crate) fn recovered_types(&self) -> impl Iterator<Item = &RecoveredType> {
        self.by_hash.values()
    }

    pub fn resolve_scene<'a>(&self, scene: &'a SceneFile) -> Result<ResolvedScene<'a>> {
        let source = scene.archetypes()?;
        let chunks = scene.chunks()?;
        let buffer_size = chunks
            .first()
            .map_or(rrising_ecs::K_CHUNK_BUFFER_SIZE, |chunk| {
                chunk.buffer.len() as i32
            });
        let mut registry = TypeManager::default();
        registry.register(TypeSpec::tag("<reserved null>"))?;
        let entity = registry.register(self.by_hash[&ENTITY_HASH].spec.clone())?;
        let mut indices = BTreeMap::from([(ENTITY_HASH, entity)]);
        for (&hash, recovered) in &self.by_hash {
            if hash != ENTITY_HASH {
                indices.insert(hash, registry.register(recovered.spec.clone())?);
            }
        }
        let special = SpecialTypes {
            entity_type: entity,
            ..Default::default()
        };
        let mut archetypes = Vec::with_capacity(source.archetypes.len());
        let mut missing = Vec::new();
        for (arch_index, arch) in source.archetypes.iter().enumerate() {
            let hashes = arch
                .types
                .iter()
                .map(|slot| source.unique_type_hashes[usize::from(slot.unique_index)])
                .collect::<Vec<_>>();
            let unresolved = hashes
                .iter()
                .copied()
                .filter(|hash| !indices.contains_key(hash))
                .collect::<Vec<_>>();
            if !unresolved.is_empty() {
                missing.push(UnresolvedArchetype {
                    index: arch_index,
                    missing_hashes: unresolved,
                });
                archetypes.push(None);
                continue;
            }
            let mut types = vec![entity];
            for (slot, hash) in arch.types.iter().zip(&hashes) {
                ensure!(
                    *hash != ENTITY_HASH,
                    "archetype {arch_index} explicitly repeats implicit Entity"
                );
                let mut index = indices[hash];
                if slot.chunk_component {
                    index.value |=
                        (TypeIndex::K_CHUNK_COMPONENT_MASK | TypeIndex::K_ZERO_SIZED_MASK) as i32;
                }
                types.push(index);
            }
            let data =
                create_archetype_data_for_buffer(&types, registry.infos(), &special, buffer_size)
                    .with_context(|| format!("layout for archetype {arch_index}"))?;
            let mut all_hashes = vec![ENTITY_HASH];
            all_hashes.extend(hashes);
            archetypes.push(Some(ResolvedArchetype {
                hashes: all_hashes,
                types,
                data,
            }));
        }
        let patches = scene.buffer_data()?;
        ensure!(
            patches.per_chunk.len() == chunks.len(),
            "buffer patch/chunk count mismatch"
        );
        let mut summary = LayoutValidation {
            total_archetypes: source.archetypes.len(),
            resolved_archetypes: archetypes.iter().filter(|a| a.is_some()).count(),
            unresolved_archetypes: missing,
            total_chunks: chunks.len(),
            ..Default::default()
        };
        let mut ids = BTreeSet::new();
        for chunk in &chunks {
            let arch_index = usize::try_from(chunk.header.archetype_index)
                .context("negative archetype index")?;
            let arch = archetypes
                .get(arch_index)
                .context("chunk references absent archetype")?;
            let Some(arch) = arch else {
                summary.unresolved_chunks += 1;
                continue;
            };
            ensure!(
                chunk.header.count <= arch.data.layout.chunk_capacity,
                "chunk {}: count {} exceeds reconstructed capacity {}",
                chunk.index,
                chunk.header.count,
                arch.data.layout.chunk_capacity
            );
            summary.resolved_chunks += 1;
            summary.resolved_entities += chunk.header.count as usize;
            let mut expected_buffer_offsets = BTreeMap::new();
            for entity_index in 0..chunk.header.count as usize {
                let bytes = slice_component(chunk, arch, 0, entity_index)?;
                let id = Entity {
                    index: i32::from_le_bytes(bytes[0..4].try_into()?),
                    version: i32::from_le_bytes(bytes[4..8].try_into()?),
                };
                ensure!(
                    id.index >= 0 && id.version >= 0,
                    "chunk {} entity {entity_index}: invalid reconstructed Entity {id:?}",
                    chunk.index
                );
                ensure!(
                    ids.insert((id.index, id.version)),
                    "chunk {} entity {entity_index}: repeated reconstructed Entity {id:?}",
                    chunk.index
                );
                for (slot, &type_index) in arch.types.iter().enumerate() {
                    if !type_index.is_buffer() || type_index.is_chunk_component() {
                        continue;
                    }
                    let bytes = slice_component(chunk, arch, slot, entity_index)?;
                    ensure!(bytes.len() >= 16, "buffer slot shorter than BufferHeader");
                    ensure!(
                        bytes[0..8] == [0; 8],
                        "chunk {} archetype {arch_index} slot {slot} entity {entity_index} type {} offset {} stride {}: serialized pointer was not cleared ({:02x?})",
                        chunk.index, registry.infos()[type_index.index() as usize].name, arch.data.layout.offsets[slot], arch.data.layout.sizeofs[slot], &bytes[..16]
                    );
                    let length = i32::from_le_bytes(bytes[8..12].try_into()?);
                    let capacity = i32::from_le_bytes(bytes[12..16].try_into()?);
                    ensure!(length >= 0 && capacity >= length, "chunk {} slot {slot} entity {entity_index}: buffer length {length}/capacity {capacity}", chunk.index);
                    let info = &registry.infos()[type_index.index() as usize];
                    let at = arch.data.layout.offsets[slot] as usize
                        + entity_index * usize::from(arch.data.layout.sizeofs[slot]);
                    expected_buffer_offsets
                        .insert(at, (capacity, info.element_size, info.buffer_capacity));
                    summary.buffer_headers += 1;
                }
            }
            let external = &patches.per_chunk[chunk.index].patches;
            let mut external_offsets = BTreeSet::new();
            for patch in external {
                let at = usize::try_from(patch.chunk_offset)?;
                let (capacity, size, _) = expected_buffer_offsets.get(&at).with_context(|| {
                    format!(
                        "chunk {} archetype {arch_index}: buffer patch at {at} misses reconstructed buffer headers; slots {:?}",
                        chunk.index,
                        arch.types.iter().enumerate().map(|(slot, t)| (slot, registry.infos()[t.index() as usize].name.clone(), arch.data.layout.offsets[slot], arch.data.layout.sizeofs[slot])).collect::<Vec<_>>()
                    )
                })?;
                ensure!(capacity.checked_mul(*size) == Some(patch.alloc_size_bytes), "chunk {}: patch at {at} is {} bytes, reconstructed capacity {capacity} × element size {size}",chunk.index,patch.alloc_size_bytes);
                ensure!(external_offsets.insert(at), "repeated buffer patch at {at}");
                summary.external_buffer_patches += 1;
            }
            for (at, (capacity, _, inline)) in expected_buffer_offsets {
                ensure!(external_offsets.contains(&at) || capacity <= inline, "chunk {} buffer at {at}: external capacity {capacity} without patch, inline {inline}",chunk.index);
            }
        }
        Ok(ResolvedScene {
            source,
            archetypes,
            chunks,
            summary,
        })
    }
}

#[derive(Clone, Debug)]
pub struct UnresolvedArchetype {
    pub index: usize,
    pub missing_hashes: Vec<u64>,
}
#[derive(Clone, Debug, Default)]
pub struct LayoutValidation {
    pub total_archetypes: usize,
    pub resolved_archetypes: usize,
    pub unresolved_archetypes: Vec<UnresolvedArchetype>,
    pub total_chunks: usize,
    pub resolved_chunks: usize,
    pub unresolved_chunks: usize,
    pub resolved_entities: usize,
    pub buffer_headers: usize,
    pub external_buffer_patches: usize,
}
#[derive(Clone, Debug)]
pub struct ResolvedArchetype {
    pub hashes: Vec<u64>,
    pub types: Vec<TypeIndex>,
    pub data: ArchetypeData,
}
pub struct ResolvedScene<'a> {
    pub source: SceneArchetypes,
    pub archetypes: Vec<Option<ResolvedArchetype>>,
    pub chunks: Vec<SceneChunk<'a>>,
    pub summary: LayoutValidation,
}

pub(crate) struct EnabledSceneRows {
    data: SceneEnabledBits,
    starts: Vec<usize>,
}

impl EnabledSceneRows {
    pub(crate) fn read(scene: &SceneFile, resolved: &ResolvedScene<'_>) -> Result<Self> {
        let data = scene.enabled_bits()?;
        let mut next = Vec::with_capacity(data.chunk_counts.len());
        let mut offset = 0;
        for (&chunks, &width) in data.chunk_counts.iter().zip(&data.component_counts) {
            next.push(offset);
            offset += chunks * width;
        }
        let starts = resolved
            .chunks
            .iter()
            .map(|chunk| {
                let archetype = usize::try_from(chunk.header.archetype_index)?;
                let start = next
                    .get_mut(archetype)
                    .context("missing scene enable archetype")?;
                let base = *start;
                *start += data.component_counts[archetype];
                Ok(base)
            })
            .collect::<Result<_>>()?;
        Ok(Self { data, starts })
    }

    pub(crate) fn get(&self, chunk: usize, row: usize, slot: usize, count: i32) -> Result<bool> {
        let index = self.starts[chunk] + slot;
        let mask = self
            .data
            .masks
            .get(index)
            .context("missing scene enable mask")?;
        let disabled = self.data.hierarchical[index];
        ensure!(row < 128 && disabled >= 0 && mask.enabled_count() as i32 + disabled == count,
            "inconsistent scene enable mask: chunk {chunk}, slot {slot}, count {count}, mask {mask:?}, disabled {disabled}");
        Ok(mask.is_enabled(row as i32))
    }
}

fn slice_component<'a>(
    chunk: &SceneChunk<'a>,
    arch: &ResolvedArchetype,
    slot: usize,
    entity: usize,
) -> Result<&'a [u8]> {
    ensure!(entity < chunk.header.count as usize, "entity outside chunk");
    let offset = usize::try_from(
        *arch
            .data
            .layout
            .offsets
            .get(slot)
            .context("component slot missing")?,
    )?;
    let stride = usize::from(
        *arch
            .data
            .layout
            .sizeofs
            .get(slot)
            .context("component stride missing")?,
    );
    let start = entity
        .checked_mul(stride)
        .and_then(|n| n.checked_add(offset))
        .context("component offset overflow")?;
    let end = start
        .checked_add(stride)
        .context("component extent overflow")?;
    chunk
        .buffer
        .get(start..end)
        .context("reconstructed component range outside chunk")
}

impl<'a> ResolvedScene<'a> {
    /// The used range of an inline or patched external buffer, without its serialized header.
    pub fn buffer_payload(
        &self,
        buffers: &crate::entity_scenes::SceneBufferData<'a>,
        chunk_index: usize,
        entity: usize,
        stable_hash: u64,
        element_size: usize,
    ) -> Result<Option<&'a [u8]>> {
        let chunk = self
            .chunks
            .get(chunk_index)
            .context("chunk index missing")?;
        let Some(arch) = self
            .archetypes
            .get(chunk.header.archetype_index as usize)
            .context("archetype index missing")?
        else {
            return Ok(None);
        };
        let Some(slot) = arch.hashes.iter().position(|&hash| hash == stable_hash) else {
            return Ok(None);
        };
        ensure!(
            arch.types[slot].is_buffer() && !arch.types[slot].is_chunk_component(),
            "requested scene component is not a per-entity buffer"
        );
        ensure!(element_size > 0, "zero-size buffer elements");
        let bytes = slice_component(chunk, arch, slot, entity)?;
        let length = usize::try_from(i32::from_le_bytes(
            bytes
                .get(8..12)
                .context("truncated scene buffer header")?
                .try_into()?,
        ))?;
        let offset = usize::try_from(arch.data.layout.offsets[slot])?
            + entity * usize::from(arch.data.layout.sizeofs[slot]);
        let patches = &buffers
            .per_chunk
            .get(chunk.index)
            .context("missing scene buffer patches")?
            .patches;
        let external = patches
            .iter()
            .find(|patch| patch.chunk_offset as usize == offset);
        let data = if let Some(patch) = external {
            patch.data
        } else {
            bytes.get(16..).context("truncated inline scene buffer")?
        };
        let used = length
            .checked_mul(element_size)
            .context("scene buffer length overflow")?;
        Ok(Some(
            data.get(..used)
                .context("used scene buffer range exceeds storage")?,
        ))
    }

    /// Returns only per-entity storage in an admitted archetype. No pointer chasing.
    pub fn component_bytes(
        &self,
        chunk: usize,
        entity: usize,
        stable_hash: u64,
    ) -> Result<Option<&'a [u8]>> {
        let chunk = self.chunks.get(chunk).context("chunk index missing")?;
        let arch = self
            .archetypes
            .get(chunk.header.archetype_index as usize)
            .context("archetype index missing")?;
        let Some(arch) = arch else {
            return Ok(None);
        };
        let Some(slot) = arch.hashes.iter().position(|&hash| hash == stable_hash) else {
            return Ok(None);
        };
        if arch.types[slot].is_chunk_component()
            || arch.types[slot].is_shared_component_type()
            || arch.types[slot].is_zero_sized()
            // The 4-byte slot is a managed-store index; order among managed types is not verified.
            || arch.types[slot].is_managed_component()
        {
            return Ok(None);
        }
        Ok(Some(slice_component(chunk, arch, slot, entity)?))
    }

    pub fn component_f32s<const N: usize>(
        &self,
        chunk: usize,
        entity: usize,
        stable_hash: u64,
    ) -> Result<Option<[f32; N]>> {
        let Some(bytes) = self.component_bytes(chunk, entity, stable_hash)? else {
            return Ok(None);
        };
        ensure!(
            bytes.len() == N * 4,
            "component {stable_hash:016x} is {} bytes, not {} floats",
            bytes.len(),
            N
        );
        let mut result = [0.; N];
        for (i, value) in result.iter_mut().enumerate() {
            *value = f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into()?);
        }
        Ok(Some(result))
    }
}
