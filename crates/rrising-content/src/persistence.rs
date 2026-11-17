//! The outer version-2 gzip persistence container, not a restored game World.
//!
//! Original server identity: `reference.server.lock.json`. `RawPersistentSaveData`
//! reads signed Int32 version/count/length words and BinaryReader UTF-8 strings,
//! then member payloads in table order. ReadVersion2 RVA 0x7f1e9e0;
//! WriteAndZipToDiskJob.Execute 0x7f31d70, WriteFileEntry 0x7f32cb0.
//! Two actual original autosaves establish the 172-byte table, seven members and
//! valid empty fourth serialization job. Header/world/system/job contents stay
//! opaque here. Unlike the original reader, this diagnostic rejects incomplete
//! payloads, trailing data and invalid UTF-8, and bounds decompression explicitly.

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

use anyhow::{ensure, Context, Result};
use flate2::bufread::GzDecoder;

/// Diagnostic allocation limit, not a limit observed in the original game.
pub const DEFAULT_MAX_DECOMPRESSED_BYTES: usize = 512 * 1024 * 1024;
pub const CONTAINER_VERSION: i32 = 2;
const REQUIRED: [&str; 3] = ["Header.save", "WorldData.save", "Systems.save"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveMember {
    pub name: String,
    /// Byte offset in the complete **decompressed** container.
    pub offset: usize,
    pub size: usize,
}

/// Owns the decompressed container once; members are slices, not extracted files.
#[derive(Debug)]
pub struct PersistentSave {
    bytes: Vec<u8>,
    members: Vec<SaveMember>,
    table_bytes: usize,
}

impl PersistentSave {
    pub fn open(path: &Path) -> Result<Self> {
        let mut file = std::fs::File::open(path)
            .with_context(|| format!("opening original save {}", path.display()))?;
        // Original LoadSaveDataAtPath selects gzip by case-sensitive suffix.
        if path.as_os_str().as_encoded_bytes().ends_with(b".gz") {
            Self::read_gzip(file, DEFAULT_MAX_DECOMPRESSED_BYTES)
        } else {
            Self::from_decompressed(read_bounded(&mut file, DEFAULT_MAX_DECOMPRESSED_BYTES)?)
        }
        .with_context(|| format!("reading save container {}", path.display()))
    }

    /// Reads one complete gzip member, verifies its checksum and enforces a
    /// caller-selected uncompressed byte limit before parsing the member table.
    pub fn read_gzip(reader: impl Read, max_decompressed_bytes: usize) -> Result<Self> {
        let mut decoder = GzDecoder::new(BufReader::new(reader));
        let bytes = read_bounded(&mut decoder, max_decompressed_bytes)
            .context("decompressing save gzip")?;
        ensure!(
            decoder.into_inner().fill_buf()?.is_empty(),
            "trailing compressed data after save gzip member"
        );
        Self::from_decompressed(bytes)
    }

    pub fn from_decompressed(bytes: Vec<u8>) -> Result<Self> {
        let mut cursor = Cursor {
            bytes: &bytes,
            offset: 0,
        };
        let version = cursor.int32()?;
        ensure!(
            version == CONTAINER_VERSION,
            "unsupported save container version {version}"
        );
        let count = cursor.nonnegative_int32("member count")?;
        // Each table entry needs at least one string-length byte and four size
        // bytes. Validate before allocating from an untrusted count.
        ensure!(
            count <= (bytes.len() - cursor.offset) / 5,
            "save member count exceeds table data"
        );
        let mut members = Vec::with_capacity(count);
        let mut names = BTreeSet::new();
        for _ in 0..count {
            let name_len = cursor.string_length()?;
            let name = std::str::from_utf8(cursor.take(name_len)?)
                .context("save member name is not valid UTF-8")?
                .to_owned();
            // ReadVersion2 uses Dictionary.Add, not replacement on collision.
            ensure!(names.insert(name.clone()), "duplicate save member {name}");
            let size = cursor.nonnegative_int32("member size")?;
            members.push(SaveMember {
                name,
                offset: 0,
                size,
            });
        }
        let table_bytes = cursor.offset;
        for member in &mut members {
            member.offset = cursor.offset;
            cursor
                .take(member.size)
                .with_context(|| format!("incomplete save member {}", member.name))?;
        }
        ensure!(
            cursor.offset == bytes.len(),
            "trailing decompressed save bytes"
        );
        for required in REQUIRED {
            ensure!(
                members.iter().any(|m| m.name == required),
                "missing required save member {required}"
            );
        }
        ensure!(
            members
                .iter()
                .any(|m| m.name.starts_with("SerializationJob_")),
            "missing save serialization job"
        );
        Ok(Self {
            bytes,
            members,
            table_bytes,
        })
    }

    pub fn members(&self) -> &[SaveMember] {
        &self.members
    }

    pub fn table_bytes(&self) -> usize {
        self.table_bytes
    }

    pub fn uncompressed_bytes(&self) -> usize {
        self.bytes.len()
    }

    /// Keeps table order; no member is written to disk.
    pub fn member_data(&self, index: usize) -> Option<&[u8]> {
        let m = self.members.get(index)?;
        self.bytes.get(m.offset..m.offset + m.size)
    }
}

fn read_bounded(reader: &mut impl Read, maximum: usize) -> Result<Vec<u8>> {
    let limit = maximum
        .checked_add(1)
        .context("save decompression limit overflows")?;
    let mut bytes = Vec::new();
    reader.take(limit as u64).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= maximum,
        "save exceeds byte limit of {maximum}"
    );
    Ok(bytes)
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, size: usize) -> Result<&'a [u8]> {
        let end = self
            .offset
            .checked_add(size)
            .context("save byte range overflows")?;
        let value = self
            .bytes
            .get(self.offset..end)
            .context("truncated save container")?;
        self.offset = end;
        Ok(value)
    }

    fn int32(&mut self) -> Result<i32> {
        let b = self.take(4)?;
        Ok(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn nonnegative_int32(&mut self, field: &str) -> Result<usize> {
        let value = self.int32()?;
        ensure!(value >= 0, "negative save {field}");
        Ok(value as usize)
    }

    /// BinaryReader.ReadString uses a 7-bit encoded signed Int32 byte count.
    fn string_length(&mut self) -> Result<usize> {
        let mut length = 0u32;
        for shift in (0..35).step_by(7) {
            let byte = self.take(1)?[0];
            ensure!(shift != 28 || byte < 8, "invalid save string length");
            length |= u32::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(length as usize);
            }
        }
        anyhow::bail!("invalid save string length")
    }
}
