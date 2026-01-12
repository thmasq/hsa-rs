use super::ApertureAllocator;
use std::collections::BTreeMap;

/// Represents a managed range of Virtual Address space.
/// Closely mirrors `manageable_aperture_t` in `fmm.c`.
#[derive(Debug)]
pub struct Aperture {
    base: u64,
    limit: u64,
    align: u64,
    guard_pages: u64,
    allocations: BTreeMap<u64, u64>,
}

impl Aperture {
    #[must_use]
    pub const fn new(base: u64, limit: u64, align: u64, guard_pages: u64) -> Self {
        Self {
            base,
            limit,
            align,
            guard_pages,
            allocations: BTreeMap::new(),
        }
    }

    const fn align_up(val: u64, align: u64) -> u64 {
        (val + align - 1) & !(align - 1)
    }
}

impl ApertureAllocator for Aperture {
    fn bounds(&self) -> (u64, u64) {
        (self.base, self.limit)
    }

    /// Port of `reserved_aperture_allocate_aligned` from `fmm.c`
    fn allocate_va(&mut self, size: usize, align: usize) -> Option<u64> {
        let size = size as u64;
        let align = std::cmp::max(align as u64, self.align);
        let guard_size = self.guard_pages * 4096;

        let request_size = size + (guard_size * 2);

        let mut candidate_start = Self::align_up(self.base, align);

        for (&alloc_start, &alloc_size) in &self.allocations {
            let alloc_end = alloc_start + alloc_size;

            if alloc_start > candidate_start {
                let gap = alloc_start - candidate_start;
                if gap >= request_size {
                    self.allocations.insert(candidate_start, request_size);
                    return Some(candidate_start + guard_size);
                }
            }

            candidate_start = Self::align_up(alloc_end, align);
        }

        if candidate_start + request_size <= self.limit {
            self.allocations.insert(candidate_start, request_size);
            return Some(candidate_start + guard_size);
        }

        None
    }

    fn free_va(&mut self, addr: u64, _size: usize) {
        let guard_size = self.guard_pages * 4096;
        let tracked_start = addr - guard_size;

        if self.allocations.remove(&tracked_start).is_none() {
            eprintln!("FMM Error: Tried to free VA 0x{addr:x} which was not tracked");
        }
    }
}
