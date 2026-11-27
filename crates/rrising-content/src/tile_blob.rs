//! Read the bounded dense collision flag arrays of a local TileBlob.
//! World placement, height levels and collision response are not implemented here.
use anyhow::{ensure, Context, Result};

pub const TILE_DATA_HASH: u64 = 0xda37_a3aa_1b2b_b1fc;

pub struct DenseCollisionTiles<'a> {
    pub flags: &'a [u8],
    pub exists: &'a [u8],
    pub bounds: [i32; 4],
}

impl<'a> DenseCollisionTiles<'a> {
    /// Read a local tile. None means outside the array or absent, not passable world space.
    pub fn flags_at(&self, tile: [i32; 2]) -> Option<u8> {
        let [x0, z0, x1, z1] = self.bounds.map(i64::from);
        let x = i64::from(tile[0]);
        let z = i64::from(tile[1]);
        if x < x0 || x >= x1 || z < z0 || z >= z1 {
            return None;
        }
        let index = usize::try_from((z - z0).checked_mul(x1 - x0)?.checked_add(x - x0)?).ok()?;
        if *self.exists.get(index)? == 0 {
            return None;
        }
        self.flags.get(index).copied()
    }

    /// TileBlob.CollisionTiles2D starts at 0x170 in the pinned client metadata.
    pub fn read(blob: &'a [u8]) -> Result<Self> {
        ensure!(blob.len() >= 0x190, "truncated TileBlob collision data");
        let array = |at: usize| -> Result<&'a [u8]> {
            let relative = i32::from_le_bytes(blob[at..at + 4].try_into()?);
            let count = i32::from_le_bytes(blob[at + 4..at + 8].try_into()?);
            ensure!(count >= 0, "negative collision array length");
            if count == 0 {
                return Ok(&[]);
            }
            let start = usize::try_from(at as i64 + i64::from(relative))
                .context("negative collision array offset")?;
            let end = start
                .checked_add(count as usize)
                .context("collision array size overflow")?;
            blob.get(start..end).context("collision array outside blob")
        };
        let flags = array(0x170)?;
        let exists = array(0x178)?;
        let bounds = std::array::from_fn(|i| {
            i32::from_le_bytes(
                blob[0x180 + i * 4..0x184 + i * 4]
                    .try_into()
                    .expect("checked root size"),
            )
        });
        ensure!(
            flags.len() == exists.len(),
            "collision array lengths differ"
        );
        if !flags.is_empty() {
            let [x0, z0, x1, z1] = bounds.map(i64::from);
            let width = x1 - x0;
            let height = z1 - z0;
            ensure!(
                width > 0 && height > 0 && width.checked_mul(height) == Some(flags.len() as i64),
                "collision bounds do not match array size"
            );
        }
        Ok(Self {
            flags,
            exists,
            bounds,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SparseCollisionTile {
    pub tile: [i32; 2],
    pub flags: u8,
}

/// Baked sparse records are int2 followed by one flag byte, with a 12-byte stride.
/// The installed sparse/dense equivalence survey establishes this bounded format.
pub fn sparse_collision_tiles(blob: &[u8]) -> Result<Vec<SparseCollisionTile>> {
    ensure!(blob.len() >= 0x170, "truncated sparse collision descriptor");
    let word = |at| i32::from_le_bytes(blob[at..at + 4].try_into().expect("checked descriptor"));
    let count = word(0x16c);
    ensure!(count >= 0, "negative sparse collision count");
    if count == 0 {
        return Ok(Vec::new());
    }
    let start =
        usize::try_from(0x168i64 + i64::from(word(0x168))).context("negative sparse offset")?;
    let size = (count as usize)
        .checked_mul(12)
        .context("sparse size overflow")?;
    let end = start.checked_add(size).context("sparse end overflow")?;
    let data = blob
        .get(start..end)
        .context("sparse collision array outside blob")?;
    Ok(data
        .as_chunks::<12>()
        .0
        .iter()
        .map(|entry| SparseCollisionTile {
            tile: [
                i32::from_le_bytes(entry[..4].try_into().expect("record size")),
                i32::from_le_bytes(entry[4..8].try_into().expect("record size")),
            ],
            flags: entry[8],
        })
        .collect())
}
