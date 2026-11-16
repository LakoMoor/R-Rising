//! The shipped `RuntimeContentCatalogData` blob, not the runtime's native hash maps.
//!
//! Evidence: `RuntimeContentCatalog.LoadCatalogData` @ RVA 0x900ffe0 loads a
//! version-1 BlobAssetReference, then its overload @ 0x90100e0 populates runtime
//! maps from five relative BlobArrays. `AddArchiveLocations` @ 0x9010ae0 reads
//! 16-byte archive IDs; `AddFileLocations` @ 0x9010f50 reads 24-byte file records;
//! `AddObjectLocations` @ 0x9011a20 reads 64-byte object records;
//! `AddSceneLocations` @ 0x9011460 reads 184-byte scene records; and
//! `AddFileDependencies` @ 0x9010690 reads nested arrays of file indices.
//!
//! Observed in the supplied build: the envelope is 4 version bytes + a 32-byte
//! BlobAssetHeader, followed by the 40-byte root. Relative offsets are measured
//! from their own four-byte offset field. All ranges and cross references below
//! are checked, and unreferenced builder alignment bytes must be zero. The blob
//! header hash is retained but its hash algorithm is not verified here. Reading
//! this catalog does not implement content streaming or gameplay.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{ensure, Context, Result};
use rrising_ecs::Hash128;

const ROOT: usize = 36;
const ROOT_SIZE: usize = 40;

#[derive(Clone, Debug)]
pub struct FileLocation {
    pub id: Hash128,
    pub archive_index: usize,
    pub dependency_set_index: usize,
}

#[derive(Clone, Debug)]
pub struct ObjectLocation {
    /// The serialized 48-byte RuntimeGlobalObjectId. Kept opaque; no guessed
    /// interpretation of its field names or padding is required for lookup.
    pub object_id: [u8; 48],
    pub file_index: usize,
    /// Preserved word at +52; AddObjectLocations does not use it.
    pub reserved: u32,
    pub local_identifier: i64,
}

#[derive(Clone, Debug)]
pub struct SceneLocation {
    pub object_id: [u8; 48],
    pub file_index: usize,
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct RuntimeContentCatalog {
    pub blob_hash: u64,
    pub archives: Vec<Hash128>,
    pub files: Vec<FileLocation>,
    pub objects: Vec<ObjectLocation>,
    pub scenes: Vec<SceneLocation>,
    pub dependency_sets: Vec<Vec<usize>>,
    /// Alignment/unreferenced bytes verified to be zero (28 in this install).
    pub padding_bytes: usize,
}

impl RuntimeContentCatalog {
    pub fn open(path: &Path) -> Result<Self> {
        let bytes =
            std::fs::read(path).with_context(|| format!("reading catalog {}", path.display()))?;
        Self::parse(&bytes).with_context(|| format!("catalog {}", path.display()))
    }

    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let mut reader = Reader {
            bytes,
            ranges: vec![(0, ROOT + ROOT_SIZE)],
        };
        ensure!(
            bytes.len() >= ROOT + ROOT_SIZE,
            "catalog header/root is truncated"
        );
        ensure!(
            reader.i32(0)? == 1,
            "unsupported catalog serialization version"
        );
        ensure!(
            reader.u64(4)? == 0,
            "catalog validation pointer must be cleared"
        );
        let length = reader.index(12)?;
        ensure!(
            length == bytes.len() - ROOT,
            "catalog blob length differs from file length"
        );
        ensure!(
            reader.i32(16)? == 1,
            "catalog serialized allocator must be 1"
        );
        ensure!(reader.u64(28)? == 0, "catalog header padding must be zero");
        let blob_hash = reader.u64(20)?;
        let archive_range = reader.array(ROOT, 16)?;
        let file_range = reader.array(ROOT + 8, 24)?;
        let object_range = reader.array(ROOT + 16, 64)?;
        let scene_range = reader.array(ROOT + 24, 184)?;
        let dependencies_range = reader.array(ROOT + 32, 8)?;

        let mut archives = Vec::with_capacity(archive_range.1);
        let mut archive_ids = BTreeSet::new();
        for i in 0..archive_range.1 {
            let id = reader.hash(archive_range.0 + 16 * i)?;
            ensure!(archive_ids.insert(id), "duplicate archive ID at index {i}");
            archives.push(id);
        }
        let mut dependency_sets = Vec::with_capacity(dependencies_range.1);
        for i in 0..dependencies_range.1 {
            let (at, count) = reader.array(dependencies_range.0 + i * 8, 4)?;
            let mut files = Vec::with_capacity(count);
            for j in 0..count {
                let file = reader.index(at + 4 * j)?;
                ensure!(
                    file < file_range.1,
                    "dependency set {i} references missing file {file}"
                );
                files.push(file);
            }
            dependency_sets.push(files);
        }
        let mut files = Vec::with_capacity(file_range.1);
        let mut file_ids = BTreeSet::new();
        for i in 0..file_range.1 {
            let at = file_range.0 + i * 24;
            let id = reader.hash(at)?;
            ensure!(file_ids.insert(id), "duplicate file ID at index {i}");
            let archive_index = reader.index(at + 16)?;
            let dependency_set_index = reader.index(at + 20)?;
            ensure!(
                archive_index < archives.len(),
                "file {i} references missing archive {archive_index}"
            );
            ensure!(
                dependency_set_index < dependency_sets.len(),
                "file {i} references missing dependency set {dependency_set_index}"
            );
            files.push(FileLocation {
                id,
                archive_index,
                dependency_set_index,
            });
        }
        let mut objects = Vec::with_capacity(object_range.1);
        for i in 0..object_range.1 {
            let at = object_range.0 + i * 64;
            let object_id = reader.fixed(at)?;
            let file_index = reader.index(at + 48)?;
            ensure!(
                file_index < files.len(),
                "object {i} references missing file {file_index}"
            );
            objects.push(ObjectLocation {
                object_id,
                file_index,
                reserved: reader.u32(at + 52)?,
                local_identifier: reader.i64(at + 56)?,
            });
        }
        let mut scenes = Vec::with_capacity(scene_range.1);
        let mut scene_ids = BTreeSet::new();
        for i in 0..scene_range.1 {
            let at = scene_range.0 + i * 184;
            let object_id = reader.fixed(at)?;
            ensure!(
                scene_ids.insert(object_id),
                "duplicate scene ID at index {i}"
            );
            let file_index = reader.index(at + 48)?;
            ensure!(
                file_index < files.len(),
                "scene {i} references missing file {file_index}"
            );
            let name_length = u16::from_le_bytes(reader.fixed(at + 52)?) as usize;
            ensure!(
                name_length <= 125,
                "scene {i} has invalid FixedString128Bytes length"
            );
            let name = std::str::from_utf8(reader.slice(at + 54, name_length)?)
                .context("scene name is not UTF-8")?
                .to_owned();
            scenes.push(SceneLocation {
                object_id,
                file_index,
                name,
            });
        }
        let padding_bytes = reader.finish()?;
        Ok(Self {
            blob_hash,
            archives,
            files,
            objects,
            scenes,
            dependency_sets,
            padding_bytes,
        })
    }

    /// The inverse of this build's Hash128.StringToHash(guidFormatted=true).
    /// The archive loader @ 0x9010ae0 formats IDs before resolving archive paths.
    /// Every generated name is cross-checked against the supplied installation.
    pub fn archive_name(&self, archive_index: usize) -> Option<String> {
        self.archives
            .get(archive_index)
            .filter(|id| id.is_valid())
            .map(|id| {
                let mut name = String::with_capacity(32);
                const HEX: &[u8; 16] = b"0123456789abcdef";
                for byte in id.to_bytes() {
                    name.push(HEX[(byte & 15) as usize] as char);
                    name.push(HEX[(byte >> 4) as usize] as char);
                }
                name
            })
    }

    /// Archive paths in catalog array order. This is not a claim about the
    /// asynchronous runtime's request/completion order.
    pub fn archive_paths(&self, directory: &Path) -> Vec<PathBuf> {
        (0..self.archives.len())
            .filter_map(|i| self.archive_name(i))
            .map(|name| directory.join(name))
            .collect()
    }

    /// Transitive file dependencies, including the requested file, in stable
    /// depth-first discovery order. The visited set makes cycles terminate.
    /// This helper is traversal policy, not a port of runtime scheduling.
    pub fn dependency_closure(&self, file_index: usize) -> Result<Vec<usize>> {
        ensure!(
            file_index < self.files.len(),
            "missing file index {file_index}"
        );
        let mut visited = BTreeSet::new();
        let mut pending = vec![file_index];
        let mut result = Vec::new();
        while let Some(file) = pending.pop() {
            if !visited.insert(file) {
                continue;
            }
            result.push(file);
            let dependencies = &self.dependency_sets[self.files[file].dependency_set_index];
            pending.extend(dependencies.iter().rev().copied());
        }
        Ok(result)
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    ranges: Vec<(usize, usize)>,
}
impl Reader<'_> {
    fn slice(&self, at: usize, len: usize) -> Result<&[u8]> {
        let end = at
            .checked_add(len)
            .context("catalog byte range overflows")?;
        self.bytes
            .get(at..end)
            .context("catalog byte range is outside file")
    }
    fn fixed<const N: usize>(&self, at: usize) -> Result<[u8; N]> {
        let mut value = [0; N];
        value.copy_from_slice(self.slice(at, N)?);
        Ok(value)
    }
    fn i32(&self, at: usize) -> Result<i32> {
        Ok(i32::from_le_bytes(self.fixed(at)?))
    }
    fn u32(&self, at: usize) -> Result<u32> {
        Ok(u32::from_le_bytes(self.fixed(at)?))
    }
    fn i64(&self, at: usize) -> Result<i64> {
        Ok(i64::from_le_bytes(self.fixed(at)?))
    }
    fn u64(&self, at: usize) -> Result<u64> {
        Ok(u64::from_le_bytes(self.fixed(at)?))
    }
    fn index(&self, at: usize) -> Result<usize> {
        usize::try_from(self.i32(at)?).context("negative catalog length/index")
    }
    fn hash(&self, at: usize) -> Result<Hash128> {
        Ok(Hash128::from_bytes(&self.fixed(at)?))
    }
    fn array(&mut self, descriptor: usize, stride: usize) -> Result<(usize, usize)> {
        let offset = self.i32(descriptor)?;
        let count = self.index(descriptor + 4)?;
        if count == 0 {
            return Ok((descriptor, 0));
        }
        let at = i64::try_from(descriptor)?
            .checked_add(i64::from(offset))
            .context("relative catalog pointer overflows")?;
        let at = usize::try_from(at).context("relative catalog pointer is negative")?;
        ensure!(at >= ROOT + ROOT_SIZE, "catalog array overlaps root/header");
        let length = count
            .checked_mul(stride)
            .context("catalog array size overflows")?;
        self.slice(at, length)?;
        self.ranges.push((at, at + length));
        Ok((at, count))
    }
    fn finish(mut self) -> Result<usize> {
        self.ranges.sort_unstable();
        let mut cursor = 0;
        let mut padding = 0;
        for (start, end) in self.ranges {
            ensure!(start >= cursor, "catalog arrays overlap");
            ensure!(
                self.bytes[cursor..start].iter().all(|&byte| byte == 0),
                "unparsed nonzero catalog data at {cursor}"
            );
            padding += start - cursor;
            cursor = end;
        }
        ensure!(
            self.bytes[cursor..].iter().all(|&byte| byte == 0),
            "unparsed nonzero catalog tail at {cursor}"
        );
        Ok(padding + self.bytes.len() - cursor)
    }
}
