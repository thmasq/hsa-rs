pub mod aperture;
pub mod manager;

/// Represents a successful memory allocation on the GPU.
#[derive(Debug, Clone)]
pub struct Allocation {
    pub ptr: *mut u8,     // CPU Virtual Address (if mapped)
    pub size: usize,      // Size in bytes
    pub gpu_va: u64,      // GPU Virtual Address
    pub handle: u64,      // KFD Allocation Handle
    pub is_userptr: bool, // Was this imported user memory?
    pub node_id: u32,     // Physical node ID
}

/// Trait for different aperture allocation strategies (e.g., Reserved vs Mmap).
pub trait ApertureAllocator {
    /// Reserve a virtual address range within this aperture.
    fn allocate_va(&mut self, size: usize, align: usize) -> Option<u64>;

    /// Free a previously reserved virtual address range.
    fn free_va(&mut self, addr: u64, size: usize);

    /// Get the aperture's base and limit.
    fn bounds(&self) -> (u64, u64);
}

// Re-export the main manager for easy access
pub use manager::MemoryManager;
