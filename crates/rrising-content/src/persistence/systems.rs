//! The original system persistence envelope. Only WorldFrame has a typed payload here.

use std::mem::size_of;

use anyhow::{ensure, Context, Result};

const MAX_IDENTIFIER_BYTES: usize = 125;
const MAX_SYSTEM_INDEX_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug)]
pub struct SavedSystems<'a> {
    pub elapsed_time: f64,
    pub slot_count: usize,
    pub records: Vec<SavedSystemRecord<'a>>,
}

#[derive(Debug)]
pub struct SavedSystemRecord<'a> {
    pub slot_index: usize,
    /// Position of the presence bit relative to the system member.
    pub offset_bits: u64,
    pub identifier: &'a str,
    /// Byte offset of the payload relative to the system member.
    pub payload_offset: usize,
    pub payload_bits: usize,
    pub payload: &'a [u8],
}

impl<'a> SavedSystems<'a> {
    pub fn read(bytes: &'a [u8]) -> Result<Self> {
        let mut cursor = Cursor {
            bytes,
            byte_offset: 0,
            bit_in_byte: 0,
        };
        let elapsed_time = f64::from_le_bytes(cursor.take_array()?);
        let slot_count = usize::try_from(i32::from_le_bytes(cursor.take_array()?))
            .context("negative system slot count")?;
        ensure!(
            slot_count as u64 <= (bytes.len() - cursor.byte_offset) as u64 * 8,
            "system slot count exceeds remaining bits"
        );
        let mut records = Vec::new();
        for slot_index in 0..slot_count {
            let offset_bits = cursor.byte_offset as u64 * 8 + u64::from(cursor.bit_in_byte);
            if cursor.read_short(1)? == 0 {
                continue;
            }
            ensure!(
                records.len() < MAX_SYSTEM_INDEX_BYTES / size_of::<SavedSystemRecord<'_>>(),
                "system index exceeds byte limit"
            );
            let name_size = usize::from(cursor.read_short(7)?);
            ensure!(
                name_size <= MAX_IDENTIFIER_BYTES,
                "system identifier exceeds FixedString128 capacity"
            );
            let identifier = std::str::from_utf8(cursor.take_aligned(name_size)?)
                .context("system identifier is not valid UTF-8")?;
            let payload_bits = usize::try_from(i32::from_le_bytes(cursor.take_array()?))
                .context("negative system payload bit count")?;
            ensure!(
                payload_bits % 8 == 0,
                "system payload for {identifier} has an unaligned bit count"
            );
            let payload_offset = cursor.byte_offset;
            let payload = cursor
                .take_aligned(payload_bits / 8)
                .with_context(|| format!("reading saved system {identifier}"))?;
            records.push(SavedSystemRecord {
                slot_index,
                offset_bits,
                identifier,
                payload_offset,
                payload_bits,
                payload,
            });
        }
        ensure!(
            cursor.aligned_offset() == bytes.len(),
            "trailing systems member bytes"
        );
        Ok(Self {
            elapsed_time,
            slot_count,
            records,
        })
    }

    /// The saved i32 singleton value; reading it does not update the simulation clock.
    pub fn world_frame(&self) -> Result<Option<i32>> {
        let mut records = self
            .records
            .iter()
            .filter(|record| record.identifier == "WorldFrame");
        let Some(record) = records.next() else {
            return Ok(None);
        };
        ensure!(records.next().is_none(), "multiple WorldFrame records");
        let bytes = record
            .payload
            .try_into()
            .context("WorldFrame payload must contain one i32")?;
        Ok(Some(i32::from_le_bytes(bytes)))
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    byte_offset: usize,
    bit_in_byte: u8,
}

impl<'a> Cursor<'a> {
    fn aligned_offset(&self) -> usize {
        self.byte_offset + usize::from(self.bit_in_byte != 0)
    }

    fn take_aligned(&mut self, size: usize) -> Result<&'a [u8]> {
        let start = self.aligned_offset();
        let end = start
            .checked_add(size)
            .context("system data size overflows")?;
        let bytes = self
            .bytes
            .get(start..end)
            .context("truncated system data")?;
        self.byte_offset = end;
        self.bit_in_byte = 0;
        Ok(bytes)
    }

    fn take_array<const SIZE: usize>(&mut self) -> Result<[u8; SIZE]> {
        self.take_aligned(SIZE)?
            .try_into()
            .context("truncated system scalar")
    }

    fn read_short(&mut self, width: u8) -> Result<u8> {
        let first = *self
            .bytes
            .get(self.byte_offset)
            .context("truncated system record bits")?;
        let next_bit = self.bit_in_byte + width;
        let mut value = u16::from(first) >> self.bit_in_byte;
        if next_bit > 8 {
            let second = *self
                .bytes
                .get(self.byte_offset + 1)
                .context("truncated system record bits")?;
            value |= u16::from(second) << (8 - self.bit_in_byte);
        }
        self.byte_offset += usize::from(next_bit / 8);
        self.bit_in_byte = next_bit % 8;
        Ok((value as u8) & ((1 << width) - 1))
    }
}
