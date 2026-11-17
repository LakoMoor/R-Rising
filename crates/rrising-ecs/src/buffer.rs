//! `Unity.Entities.BufferHeader` — the header in front of every `DynamicBuffer` element block.
//!
//! Layout from `dump.cs`: `byte* Pointer; // 0x0`, `int Length; // 0x8`, `int Capacity; // 0xC`,
//! plus `const int kMinimumCapacity = 8`. `Pointer` being null is not "no buffer": it means the
//! data is **inline**, in the bytes that follow the header inside the block that owns it — which is
//! why `GetElementPointer` returns `header + 16` when `Pointer` is null.
//!
//! A Rust header cannot sit inside its owner's block, so the port keeps the same three fields and
//! the same null-means-inline rule, and takes the inline block as a parameter on every operation
//! that can touch the data. `is_inline()` reports exactly what the null pointer reports in the
//! original.
//!
//! The allocator is the one deliberate substitution here: the original calls
//! `Unity.Collections.Memory.Unmanaged.Array.Resize(...)`, which carries allocation labels and leak
//! tracking, and the port uses the Rust global allocator with the same size and alignment. Recorded
//! in `docs/PORT.md` rather than left implicit.

use std::alloc::{alloc, dealloc, handle_alloc_error, Layout};
use std::ptr;

use anyhow::{anyhow, Result};

/// `BufferHeader.kMinimumCapacity` (`dump.cs`): the smallest capacity the original will grow to.
pub const K_MINIMUM_CAPACITY: i32 = 8;

/// The offset of the inline data from the start of the header. `GetElementPointer` returns
/// `header + 2` in the original's 8-byte units — `Pointer` 0x0, `Length` 0x8, `Capacity` 0xC, then
/// the block starts at 16.
pub const ELEMENT_OFFSET: usize = 16;

/// `BufferHeader.TrashMode` — `dump.cs`: `TrashOldData = 0`, `RetainOldData = 1`. The bodies treat
/// `RetainOldData` as "copy the overlap" and `TrashOldData` as "the new block's contents are
/// nobody's problem" (or memset to the init pattern, when one is asked for).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum TrashMode {
    TrashOldData = 0,
    RetainOldData = 1,
}

fn layout_for(bytes: usize, alignment: i32) -> Result<Layout> {
    let align = (alignment.max(1) as usize).next_power_of_two();
    Layout::from_size_align(bytes.max(1), align).map_err(|e| {
        anyhow!("a buffer of {bytes} bytes at alignment {alignment} has no valid layout: {e}")
    })
}

/// `Memory.Unmanaged.Array.Resize(0, 0, ..., totalByteSize, alignment, 0)` — allocate a block.
fn allocate(bytes: usize, alignment: i32) -> Result<(*mut u8, Layout)> {
    let layout = layout_for(bytes, alignment)?;
    let block = unsafe { alloc(layout) };
    if block.is_null() {
        handle_alloc_error(layout);
    }
    Ok((block, layout))
}

/// `BufferHeader` — `Pointer` / `Length` / `Capacity`, with the allocation's layout remembered so
/// Rust's `dealloc` can be handed the block it was given (the original's allocator knows the size
/// from the allocation site; Rust does not).
#[repr(C)]
pub struct BufferHeader {
    pointer: *mut u8,
    length: i32,
    capacity: i32,
    layout: Option<Layout>,
}

impl BufferHeader {
    /// A header with no buffer: what a chunk's buffer slot holds before `initialize`.
    #[inline]
    pub const fn empty() -> Self {
        Self {
            pointer: ptr::null_mut(),
            length: 0,
            capacity: 0,
            layout: None,
        }
    }

    /// `BufferHeader.Initialize(header, bufferCapacity)` — src:
    /// `Unity_Entities__Unity_Entities__BufferHeader__Initialize_4` @ 0x9120bb0. Sets the capacity,
    /// zeroes the pointer and the length, and does **not** free a block that was already there —
    /// the original does the same.
    #[inline]
    pub fn initialize(&mut self, buffer_capacity: i32) {
        self.capacity = buffer_capacity;
        self.pointer = ptr::null_mut();
        self.length = 0;
        self.layout = None;
    }

    /// Whether the data is inline rather than in a heap block — the original's `Pointer == 0`.
    #[inline]
    pub fn is_inline(&self) -> bool {
        self.pointer.is_null()
    }

    /// `BufferHeader.Length`.
    #[inline]
    pub fn length(&self) -> i32 {
        self.length
    }

    /// `BufferHeader.Capacity`.
    #[inline]
    pub fn capacity(&self) -> i32 {
        self.capacity
    }

    /// `BufferHeader.GetElementPointer(header)` — src:
    /// `Unity_Entities__Unity_Entities__BufferHeader__GetElementPointer_0` @ 0x9120840: the heap
    /// block when there is one, otherwise the inline block (the original computes `header +
    /// ELEMENT_OFFSET`).
    #[inline]
    pub fn element_ptr(&self, inline_base: *mut u8) -> *mut u8 {
        if self.pointer.is_null() {
            inline_base
        } else {
            self.pointer
        }
    }

    /// The bytes the buffer currently holds: `Length * type_size` of the data area.
    pub fn data<'a>(&self, inline: &'a [u8], type_size: i32) -> &'a [u8] {
        let ts = type_size.max(0) as usize;
        let area: &'a [u8] = if self.pointer.is_null() {
            inline
        } else {
            unsafe {
                std::slice::from_raw_parts(self.pointer, (self.capacity.max(0) as usize) * ts)
            }
        };
        let used = ((self.length.max(0) as usize) * ts).min(area.len());
        &area[..used]
    }

    /// `BufferHeader.SetCapacity(header, count, typeSize, alignment, trashMode,
    /// useMemoryInitPattern, memoryInitPattern, internalCapacity)` — src:
    /// `Unity_Entities__Unity_Entities__BufferHeader__SetCapacity_2` @ 0x9120950.
    ///
    /// The body's decisions, kept in order: nothing happens when the capacity already matches;
    /// `internalCapacity < count` means the new block is a heap allocation, otherwise it is the
    /// inline block; when the target and the source are the same block nothing is copied or freed;
    /// `RetainOldData` copies `min(old, new) * typeSize` bytes (and, when an init pattern is asked
    /// for, memsets the tail beyond the old contents first); `TrashOldData` with an init pattern
    /// memsets the whole block, and without one leaves it as the allocator handed it over; a heap
    /// block that is being replaced is freed. Finally `Pointer` becomes the new block or null when
    /// the data is inline, and `Capacity` becomes the new capacity. `Length` is untouched.
    #[allow(clippy::too_many_arguments)]
    pub fn set_capacity(
        &mut self,
        new_capacity: i32,
        type_size: i32,
        alignment: i32,
        trash: TrashMode,
        use_memory_init_pattern: bool,
        memory_init_pattern: u8,
        internal_capacity: i32,
        inline: &mut [u8],
    ) -> Result<()> {
        if new_capacity == self.capacity {
            return Ok(());
        }
        let ts = type_size.max(0) as usize;
        let new_bytes = (new_capacity.max(0) as usize)
            .checked_mul(ts)
            .ok_or_else(|| {
                anyhow!("capacity {new_capacity} x element size {type_size} overflows")
            })?;
        let old_bytes = (self.capacity.max(0) as usize)
            .checked_mul(ts)
            .ok_or_else(|| {
                anyhow!(
                    "capacity {} x element size {type_size} overflows",
                    self.capacity
                )
            })?;

        let inline_base = inline.as_mut_ptr();
        let old_data = if self.pointer.is_null() {
            inline_base
        } else {
            self.pointer
        };
        let use_heap = internal_capacity < new_capacity;
        let (new_data, new_layout) = if use_heap {
            allocate(new_bytes, alignment)?
        } else {
            (inline_base, layout_for(new_bytes, alignment)?)
        };

        if new_data != old_data {
            let keep = (self.capacity.max(0).min(new_capacity.max(0)) as usize) * ts;
            if use_memory_init_pattern {
                if trash == TrashMode::RetainOldData {
                    if new_bytes > keep {
                        unsafe {
                            ptr::write_bytes(
                                new_data.add(keep),
                                memory_init_pattern,
                                new_bytes - keep,
                            )
                        };
                    }
                    if keep > 0 {
                        unsafe { ptr::copy_nonoverlapping(old_data, new_data, keep) };
                    }
                } else {
                    unsafe { ptr::write_bytes(new_data, memory_init_pattern, new_bytes) };
                }
            } else if trash == TrashMode::RetainOldData && keep > 0 {
                unsafe { ptr::copy_nonoverlapping(old_data, new_data, keep) };
            }
            // `!useMemoryInitPattern && TrashOldData`: the original neither copies nor memsets.

            if !self.pointer.is_null() {
                if let Some(old_layout) = self.layout.take() {
                    unsafe { dealloc(self.pointer, old_layout) };
                }
            }
        }

        self.pointer = if use_heap { new_data } else { ptr::null_mut() };
        self.layout = if use_heap { Some(new_layout) } else { None };
        self.capacity = new_capacity;
        let _ = old_bytes;
        Ok(())
    }

    /// `BufferHeader.EnsureCapacity(header, count, typeSize, alignment, trashMode,
    /// useMemoryInitPattern, memoryInitPattern)` — src:
    /// `Unity_Entities__Unity_Entities__BufferHeader__EnsureCapacity_1` @ 0x9120870.
    ///
    /// The body's growth rule, exactly: when `Capacity < count`, double the current capacity, take
    /// `count` instead if the doubled value is smaller, and floor the result at
    /// `kMinimumCapacity` (the body reads `if (count < 9) count = 8`). It then calls `SetCapacity`
    /// with `internalCapacity = 0`, so growing always leaves the inline block behind.
    #[allow(clippy::too_many_arguments)]
    pub fn ensure_capacity(
        &mut self,
        count: i32,
        type_size: i32,
        alignment: i32,
        trash: TrashMode,
        use_memory_init_pattern: bool,
        memory_init_pattern: u8,
        inline: &mut [u8],
    ) -> Result<()> {
        if self.capacity >= count {
            return Ok(());
        }
        let mut target = self.capacity.wrapping_mul(2);
        if count > target {
            target = count;
        }
        if target < K_MINIMUM_CAPACITY + 1 {
            target = K_MINIMUM_CAPACITY;
        }
        self.set_capacity(
            target,
            type_size,
            alignment,
            trash,
            use_memory_init_pattern,
            memory_init_pattern,
            0,
            inline,
        )
    }

    /// `BufferHeader.Assign(header, source, count, typeSize, alignment, useMemoryInitPattern,
    /// memoryInitPattern)` — src:
    /// `Unity_Entities__Unity_Entities__BufferHeader__Assign_3` @ 0x9120b10: `EnsureCapacity` with
    /// `TrashOldData`, then `MemCpy` of `count * typeSize` bytes into the data area, then
    /// `Length = count`.
    #[allow(clippy::too_many_arguments)]
    pub fn assign(
        &mut self,
        source: &[u8],
        count: i32,
        type_size: i32,
        alignment: i32,
        use_memory_init_pattern: bool,
        memory_init_pattern: u8,
        inline: &mut [u8],
    ) -> Result<()> {
        self.ensure_capacity(
            count,
            type_size,
            alignment,
            TrashMode::TrashOldData,
            use_memory_init_pattern,
            memory_init_pattern,
            inline,
        )?;
        let ts = type_size.max(0) as usize;
        let bytes = (count.max(0) as usize) * ts;
        if source.len() < bytes {
            return Err(anyhow!(
                "assigning {count} elements of {type_size} bytes from {} source bytes",
                source.len()
            ));
        }
        let dst = self.element_ptr(inline.as_mut_ptr());
        if bytes > 0 {
            unsafe { ptr::copy_nonoverlapping(source.as_ptr(), dst, bytes) };
        }
        self.length = count;
        Ok(())
    }

    /// `BufferHeader.Destroy(header)` — src:
    /// `Unity_Entities__Unity_Entities__BufferHeader__Destroy_5` @ 0x9120bd0: free the heap block if
    /// there is one, then clear the pointer and write zero over `Length` and `Capacity` together
    /// (the body stores 8 bytes at offset 8).
    pub fn destroy(&mut self) {
        if !self.pointer.is_null() {
            if let Some(layout) = self.layout.take() {
                unsafe { dealloc(self.pointer, layout) };
            }
        }
        self.pointer = ptr::null_mut();
        self.length = 0;
        self.capacity = 0;
    }

    /// `BufferHeader.FreeBufferPtr(ptr)` — src:
    /// `Unity_Entities__Unity_Entities__BufferHeader__FreeBufferPtr_6` @ 0x9120c40: free a block the
    /// caller owns. The original's allocator recovers the size from the block; Rust needs it, so the
    /// port takes the same numbers it took to allocate.
    ///
    /// # Safety
    /// `ptr` must be null or a block of `bytes` at `alignment` that is not otherwise in use.
    pub unsafe fn free_buffer_ptr(ptr: *mut u8, bytes: usize, alignment: i32) {
        if ptr.is_null() {
            return;
        }
        let layout = Layout::from_size_align(
            bytes.max(1),
            (alignment.max(1) as usize).next_power_of_two(),
        )
        .unwrap_or_else(|_| Layout::new::<u8>());
        unsafe { dealloc(ptr, layout) };
    }
}

impl Default for BufferHeader {
    fn default() -> Self {
        Self::empty()
    }
}

impl Drop for BufferHeader {
    fn drop(&mut self) {
        self.destroy();
    }
}
