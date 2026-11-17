//! The original format 7 entity version table and optional nonpersistent
//! archetype debug data. The debug records do not contain component values.

use std::mem::size_of;

use anyhow::{ensure, Context, Result};

const WORD_BYTES: usize = 4;
const HASH_BYTES: usize = 8;
const MAX_WORLD_INDEX_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug)]
pub struct SavedWorldData<'a> {
    entity_versions: &'a [u8],
    pub nonpersistent_archetypes: Option<Vec<SavedWorldArchetype<'a>>>,
    parsed_bits: u64,
    trailing_zero_bytes: usize,
}

#[derive(Debug)]
pub struct SavedWorldArchetype<'a> {
    pub offset_bits: u64,
    type_hashes: &'a [u8],
    pub chunks: Vec<SavedWorldChunk<'a>>,
}

impl SavedWorldArchetype<'_> {
    pub fn type_hashes(&self) -> impl ExactSizeIterator<Item = u64> + '_ {
        self.type_hashes
            .windows(HASH_BYTES + 1)
            .step_by(HASH_BYTES)
            .map(|encoded| {
                let low = u64::from_le_bytes([
                    encoded[0], encoded[1], encoded[2], encoded[3], encoded[4], encoded[5],
                    encoded[6], encoded[7],
                ]);
                low >> 1 | (u64::from(encoded[8] & 1) << 63)
            })
    }
}

#[derive(Debug)]
pub struct SavedWorldChunk<'a> {
    entity_indices: &'a [u8],
}

impl SavedWorldChunk<'_> {
    pub fn entity_indices(&self) -> impl ExactSizeIterator<Item = usize> + '_ {
        self.entity_indices
            .windows(WORD_BYTES + 1)
            .step_by(WORD_BYTES)
            .map(|encoded| read_unaligned_i32(encoded) as usize)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SavedWorldDataStats {
    pub archetypes: usize,
    pub chunks: usize,
    pub entities: usize,
    pub type_hashes: usize,
}

impl<'a> SavedWorldData<'a> {
    pub fn read(bytes: &'a [u8]) -> Result<Self> {
        let (size, tail) = bytes
            .split_first_chunk::<WORD_BYTES>()
            .context("truncated saved world entity capacity")?;
        let entity_capacity = usize::try_from(i32::from_le_bytes(*size))
            .context("negative saved world entity capacity")?;
        let version_bytes = entity_capacity
            .checked_mul(WORD_BYTES)
            .context("saved world entity version size overflows")?;
        let (entity_versions, debug) = tail
            .split_at_checked(version_bytes)
            .context("truncated saved world entity versions")?;
        let flag = debug.first().context("missing world debug data flag")? & 1 != 0;
        let mut cursor = DebugCursor {
            remaining: debug,
            offset: WORD_BYTES + version_bytes,
            index_budget: MAX_WORLD_INDEX_BYTES,
        };
        let nonpersistent_archetypes = if flag {
            let count = cursor.count("nonpersistent archetype count")?;
            cursor.reserve::<SavedWorldArchetype<'_>>(count, 2 * WORD_BYTES)?;
            let mut archetypes = Vec::with_capacity(count);
            for _ in 0..count {
                archetypes.push(cursor.archetype(entity_capacity)?);
            }
            Some(archetypes)
        } else {
            None
        };
        let ending = &cursor.remaining[1..];
        ensure!(
            ending.is_empty() || ending == [0; WORD_BYTES],
            "unexpected bytes after saved world data"
        );
        Ok(Self {
            entity_versions,
            nonpersistent_archetypes,
            parsed_bits: cursor.bit_offset(),
            trailing_zero_bytes: ending.len(),
        })
    }

    pub fn entity_capacity(&self) -> usize {
        self.entity_versions.len() / WORD_BYTES
    }

    pub fn entity_version(&self, index: usize) -> Option<i32> {
        self.entity_versions
            .as_chunks::<WORD_BYTES>()
            .0
            .get(index)
            .map(|encoded| i32::from_le_bytes(*encoded))
    }

    pub fn entity_versions(&self) -> impl ExactSizeIterator<Item = i32> + '_ {
        self.entity_versions
            .as_chunks::<WORD_BYTES>()
            .0
            .iter()
            .map(|encoded| i32::from_le_bytes(*encoded))
    }

    /// End of the counted layout, before alignment and the observed zero tail.
    pub fn parsed_bits(&self) -> u64 {
        self.parsed_bits
    }

    pub fn trailing_zero_bytes(&self) -> usize {
        self.trailing_zero_bytes
    }

    pub fn stats(&self) -> SavedWorldDataStats {
        let mut stats = SavedWorldDataStats::default();
        if let Some(archetypes) = &self.nonpersistent_archetypes {
            stats.archetypes = archetypes.len();
            for archetype in archetypes {
                stats.type_hashes += archetype.type_hashes().len();
                stats.chunks += archetype.chunks.len();
                for chunk in &archetype.chunks {
                    stats.entities += chunk.entity_indices().len();
                }
            }
        }
        stats
    }
}

struct DebugCursor<'a> {
    remaining: &'a [u8],
    offset: usize,
    index_budget: usize,
}

impl<'a> DebugCursor<'a> {
    fn bit_offset(&self) -> u64 {
        self.offset as u64 * 8 + 1
    }

    fn take(&mut self, width: usize) -> Result<&'a [u8]> {
        let data = self
            .remaining
            .get(..=width)
            .context("truncated saved world debug record")?;
        self.remaining = &self.remaining[width..];
        self.offset += width;
        Ok(data)
    }

    fn count(&mut self, label: &str) -> Result<usize> {
        usize::try_from(read_unaligned_i32(self.take(WORD_BYTES)?))
            .with_context(|| format!("negative saved world {label}"))
    }

    fn reserve<T>(&mut self, count: usize, minimum_record_bytes: usize) -> Result<()> {
        ensure!(
            count <= (self.remaining.len() - 1) / minimum_record_bytes,
            "saved world debug record count exceeds remaining data"
        );
        self.index_budget = self
            .index_budget
            .checked_sub(byte_width(count, size_of::<T>())?)
            .context("saved world debug data exceeds index byte limit")?;
        Ok(())
    }

    fn archetype(&mut self, entity_capacity: usize) -> Result<SavedWorldArchetype<'a>> {
        let offset_bits = self.bit_offset();
        let count = self.count("type hash count")?;
        let type_hashes = self.take(byte_width(count, HASH_BYTES)?)?;
        let count = self.count("chunk count")?;
        self.reserve::<SavedWorldChunk<'_>>(count, WORD_BYTES)?;
        let mut chunks = Vec::with_capacity(count);
        for _ in 0..count {
            let count = self.count("chunk entity index count")?;
            let chunk = SavedWorldChunk {
                entity_indices: self.take(byte_width(count, WORD_BYTES)?)?,
            };
            ensure!(
                chunk.entity_indices().all(|index| index < entity_capacity),
                "nonpersistent entity index exceeds saved world capacity"
            );
            chunks.push(chunk);
        }
        Ok(SavedWorldArchetype {
            offset_bits,
            type_hashes,
            chunks,
        })
    }
}

fn read_unaligned_i32(encoded: &[u8]) -> i32 {
    let low = u32::from_le_bytes([encoded[0], encoded[1], encoded[2], encoded[3]]);
    (low >> 1 | (u32::from(encoded[4] & 1) << 31)) as i32
}

fn byte_width(count: usize, element_size: usize) -> Result<usize> {
    count
        .checked_mul(element_size)
        .context("saved world debug record size overflows")
}
