pub mod aperture;
pub mod manager;

use crate::kfd::device::KfdDevice;
use crate::kfd::ioctl::UnmapMemoryFromGpuArgs;
use manager::AllocFlags;
pub use manager::MemoryManager;
use std::sync::{Arc, Mutex};

/// Type alias for the shared, thread-safe memory manager handle.
pub type ArcManager = Arc<Mutex<MemoryManager>>;

/// Represents a successful memory allocation on the GPU (RAII).
///
/// When dropped, it automatically:
/// 1. Unmaps the CPU memory (munmap).
/// 2. Reclaims the Virtual Address space from the `MemoryManager`.
/// 3. Frees the KFD allocation handle.
#[derive(Debug)] // Clone removed to enforce RAII ownership
pub struct Allocation {
    pub ptr: *mut u8,      // CPU Virtual Address (if mapped)
    pub size: usize,       // Size in bytes
    pub gpu_va: u64,       // GPU Virtual Address
    pub handle: u64,       // KFD Allocation Handle
    pub is_userptr: bool,  // Was this imported user memory?
    pub node_id: u32,      // Physical node ID
    pub flags: AllocFlags, // Allocation flags needed for correct VA reclamation

    // Internal fields for RAII cleanup
    pub(crate) device: KfdDevice,
    pub(crate) manager_handle: ArcManager,
}

unsafe impl Send for Allocation {}
unsafe impl Sync for Allocation {}

impl Allocation {
    #[must_use]
    pub const fn as_mut_ptr(&self) -> *mut u8 {
        self.ptr
    }
}

impl Drop for Allocation {
    fn drop(&mut self) {
        // 1. Munmap CPU memory if mapped
        if !self.ptr.is_null() {
            unsafe {
                libc::munmap(self.ptr.cast(), self.size);
            }
        }

        // 2. Acquire lock to reclaim resources
        match self.manager_handle.lock() {
            Ok(mut mgr) => {
                // A. Reclaim Virtual Address Space
                mgr.free_va_from_flags(self.gpu_va, self.size, &self.flags, self.node_id);

                // B. Unmap from GPU (Fix for ResourceBusy)
                // We must unmap the memory from the device before freeing the handle.
                if let Some(gpu_id) = mgr.get_gpu_id(self.node_id) {
                    let mut unmap_args = UnmapMemoryFromGpuArgs {
                        handle: self.handle,
                        device_ids_array_ptr: &raw const gpu_id as u64,
                        n_devices: 1,
                        n_success: 0,
                    };
                    // Attempt unmap. We ignore errors here (e.g. if somehow already unmapped)
                    // because we must proceed to free the handle to avoid leaking VRAM.
                    let _ = self.device.unmap_memory_from_gpu(&mut unmap_args);
                }

                // C. Free GPU resource (KFD Handle)
                if self.handle != 0
                    && let Err(e) = self.device.free_memory_of_gpu(self.handle)
                {
                    // Ignore PermissionDenied (Os { code: 1 }) as this happens
                    // for pinned resources like Event Pages during cleanup.
                    if e.raw_os_error() != Some(1) {
                        eprintln!(
                            "[Allocation::drop] Failed to free KFD handle {}: {:?}",
                            self.handle, e
                        );
                    }
                }
            }
            Err(e) => {
                eprintln!(
                    "[Allocation::drop] Failed to acquire MemoryManager lock: {e}. VA space leaked."
                );
                // Emergency cleanup attempt if lock is poisoned
                if self.handle != 0 {
                    let _ = self.device.free_memory_of_gpu(self.handle);
                }
            }
        }
    }
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
