//! Pinned format 7 serialization jobs. All entity and component payloads borrow the
//! member bytes; indexes and component columns describe saved data, not a live World.

use std::collections::BTreeMap;
use std::mem::size_of;

use anyhow::{ensure, Context, Result};
use rrising_ecs::{Entity, TypeIndex};

use super::PersistenceHeader;

const ENTITY_BYTES: usize = 8;
const MAX_JOB_INDEX_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Copy)]
struct ColumnLayout {
    element_size: usize,
    stable_type_hash: u64,
}

struct RegistryEntry {
    omit_mask: u8,
    layout: Option<ColumnLayout>,
}

/// The exact saved TypeIndex, including flags, selects component metadata.
pub struct SerializationJobRegistry {
    types: BTreeMap<TypeIndex, RegistryEntry>,
}

impl SerializationJobRegistry {
    pub fn new(header: &PersistenceHeader) -> Result<Self> {
        ensure!(
            header.serialization_format_version == 7 && header.persistence_version == 4,
            "unsupported serialization job versions {}/{}",
            header.serialization_format_version,
            header.persistence_version
        );
        let mut layouts = BTreeMap::new();
        for component in &header.persistent_components {
            let layout = ColumnLayout {
                element_size: usize::try_from(component.component_size)
                    .context("negative saved component size")?,
                stable_type_hash: component.stable_type_hash,
            };
            ensure!(
                layouts.insert(component.saved_type_index, layout).is_none(),
                "duplicate persistent type index {:#010x}",
                component.saved_type_index.value
            );
        }
        let mut types = BTreeMap::new();
        for component in &header.required_components {
            let index = component.saved_type_index;
            let layout = if index.is_shared_component_type() || index.is_zero_sized() {
                None
            } else {
                layouts.get(&index).copied()
            };
            let entry = RegistryEntry {
                omit_mask: match component.full_name.as_str() {
                    "Unity.Transforms.LocalToWorld" => 1,
                    "Unity.Transforms.LocalTransform" => 2,
                    "Unity.Transforms.Translation"
                    | "Unity.Transforms.Rotation"
                    | "Unity.Transforms.CompositeScale" => 4,
                    "ProjectM.DestroyData"
                    | "ProjectM.DestroyState"
                    | "ProjectM.PlacementDestroyData" => 8,
                    _ => 0,
                },
                layout,
            };
            ensure!(
                types.insert(index, entry).is_none(),
                "duplicate required type index {:#010x}",
                index.value
            );
        }
        Ok(Self { types })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SerializationJobStats {
    pub archetypes: usize,
    pub chunks: usize,
    pub entities: usize,
    pub columns: usize,
    pub buffer_entities: usize,
}

#[derive(Debug)]
pub struct SerializationJob<'a> {
    pub archetypes: Vec<SavedArchetype<'a>>,
}

#[derive(Debug)]
pub struct SavedArchetype<'a> {
    /// Start of the record in bits relative to its serialization job member.
    pub offset_bits: u64,
    pub types: Vec<TypeIndex>,
    pub omission_flags: u8,
    pub chunks: Vec<SavedChunk<'a>>,
}

#[derive(Debug)]
pub struct SavedChunk<'a> {
    /// Byte offset of the aligned entity array relative to its job member.
    pub entity_array_offset: usize,
    entity_bytes: &'a [u8],
    pub columns: Vec<SavedColumn<'a>>,
}

impl SavedChunk<'_> {
    pub fn entities(&self) -> impl ExactSizeIterator<Item = Entity> + '_ {
        self.entity_bytes
            .as_chunks::<ENTITY_BYTES>()
            .0
            .iter()
            .map(|record| {
                Entity::new(
                    int32([record[0], record[1], record[2], record[3]]),
                    int32([record[4], record[5], record[6], record[7]]),
                )
            })
    }
}

#[derive(Debug)]
pub struct SavedColumn<'a> {
    pub saved_type_index: TypeIndex,
    pub stable_type_hash: u64,
    /// Byte offset relative to the job member. For buffers it includes the first count.
    pub offset: usize,
    bytes: &'a [u8],
    element_size: usize,
    entity_count: usize,
    is_buffer: bool,
}

impl<'a> SavedColumn<'a> {
    pub fn element_size(&self) -> usize {
        self.element_size
    }

    /// Width of the encoded column, including buffer length prefixes.
    pub fn encoded_len(&self) -> usize {
        self.bytes.len()
    }

    pub fn scalar(&self, entity_index: usize) -> Option<&'a [u8]> {
        if self.is_buffer || entity_index >= self.entity_count {
            return None;
        }
        let start = entity_index * self.element_size;
        self.bytes.get(start..start + self.element_size)
    }

    /// Yields one contiguous element payload for every entity, in entity order.
    pub fn buffers(&self) -> Option<impl ExactSizeIterator<Item = &'a [u8]>> {
        self.is_buffer.then_some(BufferRows {
            remaining: self.bytes,
            element_size: self.element_size,
            row_count: self.entity_count,
        })
    }
}

struct BufferRows<'a> {
    remaining: &'a [u8],
    element_size: usize,
    row_count: usize,
}

impl<'a> Iterator for BufferRows<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        if self.row_count == 0 {
            return None;
        }
        let (count, payload) = self.remaining.split_first_chunk::<4>()?;
        let length = usize::try_from(int32(*count))
            .ok()?
            .checked_mul(self.element_size)?;
        let (row, remaining) = payload.split_at_checked(length)?;
        self.remaining = remaining;
        self.row_count -= 1;
        Some(row)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.row_count, Some(self.row_count))
    }
}

impl ExactSizeIterator for BufferRows<'_> {}

impl<'a> SerializationJob<'a> {
    pub fn read(bytes: &'a [u8], registry: &SerializationJobRegistry) -> Result<Self> {
        let mut cursor = JobCursor {
            bytes,
            offset: 0,
            half: false,
        };
        let mut index = IndexBudget(MAX_JOB_INDEX_BYTES);
        let mut archetypes = Vec::new();
        while cursor.offset < bytes.len() {
            let offset_bits = cursor.bit_offset();
            index.use_entries::<SavedArchetype<'_>>(1)?;
            archetypes.push(
                Self::read_archetype(&mut cursor, registry, &mut index)
                    .with_context(|| format!("archetype at job bit {offset_bits}"))?,
            );
        }
        Ok(Self { archetypes })
    }

    fn read_archetype(
        cursor: &mut JobCursor<'a>,
        registry: &SerializationJobRegistry,
        index: &mut IndexBudget,
    ) -> Result<SavedArchetype<'a>> {
        let offset_bits = cursor.bit_offset();
        let type_count = cursor.count("type count")?;
        ensure!(
            type_count <= registry.types.len() && type_count <= cursor.remaining_whole_bytes() / 4,
            "saved archetype type count exceeds the registry or remaining bytes"
        );
        index.use_entries::<TypeIndex>(type_count)?;
        let mut types = Vec::with_capacity(type_count);
        for _ in 0..type_count {
            types.push(TypeIndex::new(cursor.read_i32()?));
        }
        let omission_flags = cursor.read_nibble()?;
        let mut columns = Vec::new();
        for &saved_type_index in &types {
            let entry = registry.types.get(&saved_type_index).with_context(|| {
                format!("unknown saved type index {:#010x}", saved_type_index.value)
            })?;
            if let Some(layout) = entry.layout {
                if entry.omit_mask & omission_flags == 0 {
                    columns.push((saved_type_index, layout));
                }
            }
        }
        let chunk_count = cursor.count("chunk count")?;
        ensure!(
            chunk_count <= cursor.remaining_whole_bytes() / 4,
            "saved archetype chunk count exceeds remaining bytes"
        );
        index.use_entries::<SavedChunk<'_>>(chunk_count)?;
        index.use_entries::<SavedColumn<'_>>(
            chunk_count
                .checked_mul(columns.len())
                .context("serialization job column count overflows")?,
        )?;
        let mut chunks = Vec::with_capacity(chunk_count);
        for _ in 0..chunk_count {
            let entity_count = cursor.count("entity count")?;
            let entity_array_offset = cursor.aligned_offset();
            let entity_bytes = cursor.take_aligned(product(entity_count, ENTITY_BYTES)?)?;
            let mut saved_columns = Vec::with_capacity(columns.len());
            for &(saved_type_index, layout) in &columns {
                let offset = cursor.aligned_offset();
                let is_buffer = saved_type_index.is_buffer();
                let bytes = if is_buffer {
                    for _ in 0..entity_count {
                        let element_count = cursor.count("buffer element count")?;
                        cursor.take_aligned(product(element_count, layout.element_size)?)?;
                    }
                    &cursor.bytes[offset..cursor.offset]
                } else {
                    cursor.take_aligned(product(entity_count, layout.element_size)?)?
                };
                saved_columns.push(SavedColumn {
                    saved_type_index,
                    stable_type_hash: layout.stable_type_hash,
                    offset,
                    bytes,
                    element_size: layout.element_size,
                    entity_count,
                    is_buffer,
                });
            }
            chunks.push(SavedChunk {
                entity_array_offset,
                entity_bytes,
                columns: saved_columns,
            });
        }
        Ok(SavedArchetype {
            offset_bits,
            types,
            omission_flags,
            chunks,
        })
    }

    pub fn stats(&self) -> SerializationJobStats {
        let mut stats = SerializationJobStats {
            archetypes: self.archetypes.len(),
            ..SerializationJobStats::default()
        };
        for archetype in &self.archetypes {
            stats.chunks += archetype.chunks.len();
            for chunk in &archetype.chunks {
                stats.entities += chunk.entity_bytes.len() / ENTITY_BYTES;
                stats.columns += chunk.columns.len();
                for column in &chunk.columns {
                    if column.is_buffer {
                        stats.buffer_entities += column.entity_count;
                    }
                }
            }
        }
        stats
    }
}

/// Index budget bounds metadata allocated from untrusted record counts. Payloads do not count
/// toward this budget because they are already held by the caller.
struct IndexBudget(usize);

impl IndexBudget {
    fn use_entries<T>(&mut self, count: usize) -> Result<()> {
        self.0 = self
            .0
            .checked_sub(product(count, size_of::<T>())?)
            .context("serialization job exceeds index byte limit")?;
        Ok(())
    }
}

fn product(count: usize, byte_size: usize) -> Result<usize> {
    count
        .checked_mul(byte_size)
        .context("serialization job byte count overflows")
}

#[inline]
fn int32(bytes: [u8; 4]) -> i32 {
    i32::from_le_bytes(bytes)
}

struct JobCursor<'a> {
    bytes: &'a [u8],
    offset: usize,
    half: bool,
}

impl<'a> JobCursor<'a> {
    fn bit_offset(&self) -> u64 {
        self.offset as u64 * 8 + u64::from(self.half) * 4
    }

    fn aligned_offset(&self) -> usize {
        self.offset + usize::from(self.half)
    }

    fn remaining_whole_bytes(&self) -> usize {
        self.bytes.len() - self.aligned_offset()
    }

    fn take_aligned(&mut self, length: usize) -> Result<&'a [u8]> {
        let start = self.aligned_offset();
        let end = start
            .checked_add(length)
            .context("serialization job byte range overflows")?;
        let bytes = self
            .bytes
            .get(start..end)
            .context("truncated serialization job data")?;
        self.offset = end;
        self.half = false;
        Ok(bytes)
    }

    fn read_nibble(&mut self) -> Result<u8> {
        let byte = *self
            .bytes
            .get(self.offset)
            .context("truncated omission flags")?;
        let result = if self.half {
            self.offset += 1;
            byte >> 4
        } else {
            byte & 15
        };
        self.half = !self.half;
        Ok(result)
    }

    fn read_i32(&mut self) -> Result<i32> {
        let remaining = &self.bytes[self.offset..];
        let (first, rest) = remaining
            .split_first_chunk::<4>()
            .context("truncated serialization job word")?;
        let value = int32(*first);
        let result = if self.half {
            let last = *rest
                .first()
                .context("truncated unaligned serialization job word")?;
            ((value as u32 >> 4) | (u32::from(last & 15) << 28)) as i32
        } else {
            value
        };
        self.offset += 4;
        Ok(result)
    }

    fn count(&mut self, field: &str) -> Result<usize> {
        usize::try_from(self.read_i32()?).with_context(|| format!("negative saved {field}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::{PersistentComponent, RequiredComponent};

    const SCALAR: TypeIndex = TypeIndex::new(17);
    const BUFFER: TypeIndex = TypeIndex::pack(18, TypeIndex::K_BUFFER_MASK);
    const LOCAL_TO_WORLD: TypeIndex = TypeIndex::new(19);
    const SHARED: TypeIndex = TypeIndex::pack(20, TypeIndex::K_SHARED_MASK);
    const ZERO: TypeIndex = TypeIndex::pack(21, TypeIndex::K_ZERO_SIZED_MASK);
    const FLAGGED_SCALAR: TypeIndex = TypeIndex::pack(17, TypeIndex::K_HAS_ENTITY_REFERENCES_MASK);
    const JOB: [u8; 103] = [
        0x06, 0x00, 0x00, 0x00, 0x11, 0x00, 0x00, 0x00, 0x12, 0x00, 0x00, 0x04, 0x13, 0x00, 0x00,
        0x00, 0x14, 0x00, 0x00, 0x08, 0x15, 0x00, 0x00, 0x40, 0x11, 0x00, 0x02, 0x00, 0x11, 0x00,
        0x00, 0x00, 0x20, 0x00, 0x00, 0x00, 0xa0, 0x65, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00,
        0x66, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0xfa, 0xfb, 0xc0, 0xc1, 0x00, 0x00, 0x00,
        0x00, 0x03, 0x00, 0x00, 0x00, 0x01, 0x19, 0x02, 0x17, 0x03, 0xab, 0x02, 0x00, 0x00, 0x00,
        0x13, 0x00, 0x00, 0x00, 0x11, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00,
        0x00, 0xa0, 0x67, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0xa0, 0xb1, 0xb2,
    ];

    fn registry() -> SerializationJobRegistry {
        let persistent_components = [
            (SCALAR, 2),
            (BUFFER, 2),
            (LOCAL_TO_WORLD, 1),
            (SHARED, 4),
            (ZERO, 0),
        ]
        .map(|(saved_type_index, component_size)| PersistentComponent {
            saved_type_index,
            stable_type_hash: saved_type_index.value as u64 + 10,
            component_size,
        })
        .into();
        let required_components = [
            (SCALAR, "Scalar"),
            (BUFFER, "Buffer"),
            (LOCAL_TO_WORLD, "Unity.Transforms.LocalToWorld"),
            (SHARED, "Shared"),
            (ZERO, "Zero"),
            (FLAGGED_SCALAR, "FlaggedScalar"),
        ]
        .map(|(saved_type_index, name)| RequiredComponent {
            saved_type_index,
            full_name: name.to_owned(),
        })
        .into();
        SerializationJobRegistry::new(&PersistenceHeader {
            serialization_format_version: 7,
            persistence_version: 4,
            game_version: [0; 4],
            application_type: 0,
            conversion_target: 0,
            prefab_conversion_method: 0,
            using_server_only_subscenes: false,
            prefabs: Vec::new(),
            persistent_components,
            required_components,
            blobs: Vec::new(),
        })
        .unwrap()
    }

    #[test]
    fn aligns_chunk_words_and_reads_only_persistent_non_omitted_columns() {
        let parsed = SerializationJob::read(&JOB, &registry()).unwrap();
        assert_eq!(
            parsed.stats(),
            SerializationJobStats {
                archetypes: 2,
                chunks: 2,
                entities: 3,
                columns: 4,
                buffer_entities: 2,
            }
        );
        let first_archetype = &parsed.archetypes[0];
        assert_eq!(
            first_archetype.types,
            [SCALAR, BUFFER, LOCAL_TO_WORLD, SHARED, ZERO, FLAGGED_SCALAR]
        );
        assert_eq!(first_archetype.omission_flags, 1);
        let first_chunk = &first_archetype.chunks[0];
        assert_eq!(first_chunk.entity_array_offset, 37);
        assert_eq!(
            first_chunk.entities().collect::<Vec<_>>(),
            [Entity::new(101, 2), Entity::new(102, 3)]
        );
        let scalar = &first_chunk.columns[0];
        assert_eq!(
            (scalar.stable_type_hash, scalar.offset, scalar.encoded_len()),
            (27, 53, 4)
        );
        assert_eq!(scalar.scalar(1), Some(&[0xc0, 0xc1][..]));
        assert_eq!(scalar.scalar(2), None);
        assert!(scalar.buffers().is_none());
        let buffer = &first_chunk.columns[1];
        assert_eq!((buffer.offset, buffer.encoded_len()), (57, 14));
        assert_eq!(buffer.scalar(0), None);
        assert_eq!(
            buffer.buffers().unwrap().collect::<Vec<_>>(),
            [&[][..], &[1, 0x19, 2, 0x17, 3, 0xab]]
        );
        let second_archetype = &parsed.archetypes[1];
        assert_eq!(
            (
                second_archetype.offset_bits,
                second_archetype.omission_flags
            ),
            (568, 0)
        );
        assert_eq!(
            second_archetype.chunks[0].columns[0].scalar(0),
            Some(&[0xa0][..])
        );
    }

    #[test]
    fn accepts_empty_job_and_rejects_truncated_or_negative_buffer_records() {
        let registry = registry();
        assert_eq!(
            SerializationJob::read(&[], &registry).unwrap().stats(),
            SerializationJobStats::default()
        );
        assert!(SerializationJob::read(&JOB[..70], &registry).is_err());
        assert!(SerializationJob::read(&JOB[..102], &registry).is_err());
        let mut invalid = JOB;
        invalid[61..65].copy_from_slice(&(-1i32).to_le_bytes());
        assert!(format!(
            "{:#}",
            SerializationJob::read(&invalid, &registry).unwrap_err()
        )
        .contains("negative saved buffer element count"));
    }
}
