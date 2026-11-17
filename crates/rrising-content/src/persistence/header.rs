//! Pinned persistence header schema (format 7, persistence version 4).
//!
//! Native metadata/header reader and writer are documented in
//! `evidence/save_header.md`. Actual server autosaves were captured before this
//! decoder. Runtime TypeIndex values belong to the saved registry, not the port's
//! registry. Blob addresses are saved identifiers, never live pointers.

use anyhow::{ensure, Context, Result};
use rrising_ecs::{Entity, TypeIndex};

use super::Cursor;

#[derive(Debug)]
pub struct PrefabReference {
    pub prefab_guid: i32,
    pub entity: Entity,
    pub linked_entities: Vec<Entity>,
}

#[derive(Debug)]
pub struct PersistentComponent {
    pub stable_type_hash: u64,
    pub component_size: i32,
    pub saved_type_index: TypeIndex,
}

#[derive(Debug)]
pub struct RequiredComponent {
    pub full_name: String,
    pub saved_type_index: TypeIndex,
}

#[derive(Debug)]
pub struct PersistentBlob {
    pub entity: Entity,
    pub component_stable_type_hash: u64,
    pub blob_field_offset: i32,
    pub element_index: i32,
    /// Original saved IntPtr bits; never dereference these in the port process.
    pub saved_blob_asset_address: u64,
}

#[derive(Debug)]
pub struct PersistenceHeader {
    pub serialization_format_version: i32,
    pub persistence_version: i32,
    pub game_version: [i32; 4],
    pub application_type: u8,
    pub conversion_target: u8,
    pub prefab_conversion_method: u8,
    pub using_server_only_subscenes: bool,
    pub prefabs: Vec<PrefabReference>,
    pub persistent_components: Vec<PersistentComponent>,
    pub required_components: Vec<RequiredComponent>,
    pub blobs: Vec<PersistentBlob>,
}

impl PersistenceHeader {
    /// Reads the one header schema measured in the pinned server. Names are
    /// FixedString512Bytes: a 9-bit length (max 509), then byte alignment and UTF-8.
    pub fn read(bytes: &[u8]) -> Result<Self> {
        let mut cursor = Cursor { bytes, offset: 0 };
        let serialization_format_version = cursor.int32()?;
        let persistence_version = cursor.int32()?;
        ensure!(serialization_format_version == 7 && persistence_version == 4,
                "unsupported persistence header versions {serialization_format_version}/{persistence_version}");
        let game_version = [
            cursor.int32()?,
            cursor.int32()?,
            cursor.int32()?,
            cursor.int32()?,
        ];
        let application_type = cursor.take(1)?[0];
        let conversion_target = cursor.take(1)?[0];
        let prefab_conversion_method = cursor.take(1)?[0];
        // A single low bit, followed by original byte alignment (padding ignored).
        let using_server_only_subscenes = cursor.take(1)?[0] & 1 != 0;

        let count = cursor.record_count("prefab count", 16)?;
        let mut prefabs = Vec::with_capacity(count);
        for _ in 0..count {
            let prefab_guid = cursor.int32()?;
            let entity = cursor.entity()?;
            let linked_count = cursor.record_count("linked entity count", 8)?;
            let mut linked_entities = Vec::with_capacity(linked_count);
            for _ in 0..linked_count {
                linked_entities.push(cursor.entity()?);
            }
            prefabs.push(PrefabReference {
                prefab_guid,
                entity,
                linked_entities,
            });
        }

        let count = cursor.record_count("persistent component count", 16)?;
        let mut persistent_components = Vec::with_capacity(count);
        for _ in 0..count {
            persistent_components.push(PersistentComponent {
                stable_type_hash: cursor.uint64()?,
                component_size: cursor.int32()?,
                saved_type_index: TypeIndex::new(cursor.int32()?),
            });
        }

        let count = cursor.record_count("required component count", 6)?;
        let mut required_components = Vec::with_capacity(count);
        for _ in 0..count {
            let b = cursor.take(2)?;
            // Native reads nine bits then skips the seven alignment bits. A
            // plain u16 length incorrectly treats nonzero padding as length.
            let length = (u16::from_le_bytes([b[0], b[1]]) & 0x1ff) as usize;
            ensure!(length <= 509, "invalid FixedString512 byte length {length}");
            let full_name = std::str::from_utf8(cursor.take(length)?)
                .context("required component name is not valid UTF-8")?
                .to_owned();
            required_components.push(RequiredComponent {
                full_name,
                saved_type_index: TypeIndex::new(cursor.int32()?),
            });
        }

        let count = cursor.record_count("blob record count", 32)?;
        let mut blobs = Vec::with_capacity(count);
        for _ in 0..count {
            blobs.push(PersistentBlob {
                entity: cursor.entity()?,
                component_stable_type_hash: cursor.uint64()?,
                blob_field_offset: cursor.int32()?,
                element_index: cursor.int32()?,
                saved_blob_asset_address: cursor.uint64()?,
            });
        }
        ensure!(
            cursor.offset == bytes.len(),
            "trailing persistence header bytes"
        );
        Ok(Self {
            serialization_format_version,
            persistence_version,
            game_version,
            application_type,
            conversion_target,
            prefab_conversion_method,
            using_server_only_subscenes,
            prefabs,
            persistent_components,
            required_components,
            blobs,
        })
    }

    /// Resolves by the full saved TypeIndex, preserving its flag bits. This is
    /// metadata lookup, not registration of a port runtime component layout.
    pub fn persistent_component_named(&self, name: &str) -> Option<&PersistentComponent> {
        let index = self
            .required_components
            .iter()
            .find(|r| r.full_name == name)?
            .saved_type_index;
        self.persistent_components
            .iter()
            .find(|c| c.saved_type_index == index)
    }
}

impl Cursor<'_> {
    fn entity(&mut self) -> Result<Entity> {
        Ok(Entity::new(self.int32()?, self.int32()?))
    }

    fn uint64(&mut self) -> Result<u64> {
        let b = self.take(8)?;
        Ok(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    fn record_count(&mut self, field: &str, minimum_record_bytes: usize) -> Result<usize> {
        let count = self.nonnegative_int32(field)?;
        ensure!(
            count <= (self.bytes.len() - self.offset) / minimum_record_bytes,
            "persistence header {field} exceeds remaining bytes"
        );
        Ok(count)
    }
}
