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
        Ok(Self {
            flags,
            exists,
            bounds,
        })
    }
}
