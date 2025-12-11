#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

use crate::kfd::device::KfdDevice;
use crate::kfd::ioctl::{
    CreateQueueArgs, KFD_IOC_QUEUE_TYPE_COMPUTE, KFD_IOC_QUEUE_TYPE_COMPUTE_AQL,
    KFD_IOC_QUEUE_TYPE_SDMA, KFD_IOC_QUEUE_TYPE_SDMA_XGMI,
};
use crate::kfd::sysfs::HsaNodeProperties;
use crate::thunk::memory::Allocation;
use crate::thunk::queues::cwsr;
use std::os::fd::RawFd;
use std::ptr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueType {
    Compute = 1,
    Sdma = 2,
    ComputeAql = 21,
    SdmaXgmi = 5,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueuePriority {
    Minimum = -3,
    Low = -2,
    BelowNormal = -1,
    Normal = 0,
    AboveNormal = 1,
    High = 2,
    Maximum = 3,
}

/// A RAII-wrapper around a KFD Queue and its resources.
///
/// This struct takes ownership of the queue ID and associated memory allocations (EOP, CWSR).
/// When dropped, it automatically destroys the queue and frees the GPU memory backing the resources.
#[derive(Debug)]
pub struct HsaQueue {
    pub queue_id: u32,
    pub queue_doorbell: u64,   // Virtual address of doorbell
    pub queue_read_ptr: u64,   // Virtual address of read ptr
    pub queue_write_ptr: u64,  // Virtual address of write ptr
    pub queue_err_reason: u64, // Virtual address of error reason

    // Internal resources kept for lifetime management
    device: KfdDevice,
    eop_mem: Option<Allocation>,
    cwsr_mem: Option<Allocation>,
    ptr_mem: Option<Allocation>,
}

impl Drop for HsaQueue {
    fn drop(&mut self) {
        // 1. Destroy the hardware queue first to stop GPU access
        if let Err(e) = self.device.destroy_queue(self.queue_id) {
            eprintln!(
                "[HsaQueue] Failed to destroy queue ID {}: {:?}",
                self.queue_id, e
            );
        }

        // 2. Free associated GPU memory resources
        // Note: This calls the KFD free ioctl via the device.
        // If the MemoryManager tracks VA ranges, those ranges effectively leak here
        // unless the MemoryManager is shared/singleton. For a simple thunk, this
        // ensures the physical/backing memory is returned to the OS.
        if let Some(alloc) = &self.eop_mem {
            self.device.free_memory_of_gpu(alloc.handle).ok();
        }
        if let Some(alloc) = &self.cwsr_mem {
            self.device.free_memory_of_gpu(alloc.handle).ok();
        }
        if let Some(alloc) = &self.ptr_mem {
            self.device.free_memory_of_gpu(alloc.handle).ok();
        }
    }
}

/// Abstraction for the Flat Memory Model manager needed by the builder.
pub trait MemoryManager {
    /// Allocate GPU accessible memory (GTT or VRAM)
    fn allocate_gpu_memory(
        &mut self,
        device: &KfdDevice,
        size: usize,
        align: usize,
        vram: bool,
        public: bool,
        drm_fd: RawFd,
    ) -> Result<Allocation, i32>;

    /// Free allocated memory
    fn free_gpu_memory(&mut self, device: &KfdDevice, alloc: &Allocation);

    /// Map a doorbell index to a CPU virtual address
    fn map_doorbell(
        &mut self,
        device: &KfdDevice,
        node_id: u32,
        gpu_id: u32,
        doorbell_offset: u64,
        size: u64,
    ) -> Result<*mut u32, i32>;
}

pub struct QueueBuilder<'a> {
    device: &'a KfdDevice,
    mem_mgr: &'a mut dyn MemoryManager,
    node_props: &'a HsaNodeProperties,

    // Inputs
    node_id: u32,
    drm_fd: RawFd,
    queue_type: QueueType,
    percentage: u32,
    priority: QueuePriority,
    ring_base: u64,
    ring_size: u64,
    sdma_engine_id: u32,
}

impl<'a> QueueBuilder<'a> {
    pub fn new(
        device: &'a KfdDevice,
        mem_mgr: &'a mut dyn MemoryManager,
        node_props: &'a HsaNodeProperties,
        node_id: u32,
        drm_fd: RawFd,
        ring_base: u64,
        ring_size: u64,
    ) -> Self {
        Self {
            device,
            mem_mgr,
            node_props,
            node_id,
            drm_fd,
            ring_base,
            ring_size,
            queue_type: QueueType::Compute,
            percentage: 100,
            priority: QueuePriority::Normal,
            sdma_engine_id: 0,
        }
    }

    #[must_use]
    pub const fn with_type(mut self, t: QueueType) -> Self {
        self.queue_type = t;
        self
    }

    #[must_use]
    pub const fn with_priority(mut self, p: QueuePriority) -> Self {
        self.priority = p;
        self
    }

    /// Creates the queue in the KFD and allocates necessary resources.
    ///
    /// # Errors
    /// Returns `i32` error codes (typically -1) if allocation or IOCTLs fail.
    ///
    /// # Panics
    /// Panics if CWSR is allocated but the size calculation returns `None` during the IOCTL setup phase.
    /// This indicates an internal logic inconsistency where memory was allocated based on sizes,
    /// but the sizes are missing when needed later.
    pub fn create(mut self) -> Result<HsaQueue, i32> {
        let gfx_version = self.node_props.gfx_target_version;
        let is_compute = matches!(self.queue_type, QueueType::Compute | QueueType::ComputeAql);

        // 1. Prepare EOP (End-Of-Pipe) Buffer
        let eop_mem = self.alloc_eop(gfx_version, is_compute)?;

        // 2. Prepare CWSR (Context Save/Restore) Area
        // Only for GFX8+ (Carrizo+)
        let (cwsr_mem, cwsr_sizes) = self.alloc_cwsr(gfx_version, is_compute)?;

        // 3. Allocate Queue Read/Write Pointers (if needed)
        // These are small 64-bit counters usually placed in a 4K page.
        let ptr_mem = self.alloc_pointers()?;

        let rptr_va = ptr_mem.gpu_va;
        let wptr_va = ptr_mem.gpu_va + 8;
        let ptr_mem = Some(ptr_mem);

        // 4. Setup IOCTL Arguments
        let mut args = CreateQueueArgs {
            gpu_id: self.node_props.kfd_gpu_id,
            ring_base_address: self.ring_base,
            ring_size: self.ring_size as u32,
            queue_type: match self.queue_type {
                QueueType::Compute => KFD_IOC_QUEUE_TYPE_COMPUTE,
                QueueType::Sdma => KFD_IOC_QUEUE_TYPE_SDMA,
                QueueType::ComputeAql => KFD_IOC_QUEUE_TYPE_COMPUTE_AQL,
                QueueType::SdmaXgmi => KFD_IOC_QUEUE_TYPE_SDMA_XGMI,
            },
            queue_percentage: self.percentage,
            queue_priority: Self::map_priority(self.priority),
            sdma_engine_id: self.sdma_engine_id,
            ..Default::default()
        };

        // Pointers setup
        if self.queue_type == QueueType::ComputeAql {
            // For AQL, pointers are inside the ring buffer/packet processor.
            args.read_pointer_address = 0;
            args.write_pointer_address = 0;
        } else {
            args.read_pointer_address = rptr_va;
            args.write_pointer_address = wptr_va;
        }

        // EOP & CWSR Args
        if let Some(eop) = &eop_mem {
            args.eop_buffer_address = eop.gpu_va;
            args.eop_buffer_size = eop.size as u64;
        }
        if let Some(cwsr) = &cwsr_mem {
            let sizes = cwsr_sizes.as_ref().unwrap();
            args.ctx_save_restore_address = cwsr.gpu_va;
            args.ctx_save_restore_size = sizes.ctx_save_restore_size;
            args.ctl_stack_size = sizes.ctl_stack_size;
        }

        // 5. Call KFD CreateQueue IOCTL
        self.device.create_queue(&mut args).map_err(|e| {
            eprintln!("KFD CreateQueue failed: {e:?}");
            -1
        })?;

        // 6. Map Doorbell
        // We do this after creation because we need the doorbell_offset returned by KFD.
        let doorbell_ptr = self.resolve_doorbell_ptr(args.doorbell_offset, gfx_version)?;

        // 7. Construct RAII Result
        Ok(HsaQueue {
            queue_id: args.queue_id,
            queue_doorbell: doorbell_ptr as u64,
            queue_read_ptr: rptr_va,
            queue_write_ptr: wptr_va,
            queue_err_reason: 0,

            // Clone the device handle (cheap Arc clone) so the queue can clean itself up on Drop
            device: self.device.clone(),

            // Transfer ownership of allocations to the queue struct
            eop_mem,
            cwsr_mem,
            ptr_mem,
        })
    }

    fn alloc_eop(&mut self, gfx_version: u32, is_compute: bool) -> Result<Option<Allocation>, i32> {
        let eop_size = Self::calculate_eop_size(gfx_version, is_compute);
        if eop_size > 0 {
            let mut alloc_res = self.mem_mgr.allocate_gpu_memory(
                self.device,
                eop_size,
                4096,
                true, // Try VRAM first
                true,
                self.drm_fd,
            );

            if alloc_res.is_err() {
                // Fallback to GTT
                alloc_res = self.mem_mgr.allocate_gpu_memory(
                    self.device,
                    eop_size,
                    4096,
                    false, // VRAM=false -> GTT
                    true,
                    self.drm_fd,
                );
            }

            let alloc = alloc_res.inspect_err(|_e| {
                eprintln!("Failed to allocate EOP buffer");
            })?;

            unsafe {
                ptr::write_bytes(alloc.ptr, 0, eop_size);
            }
            Ok(Some(alloc))
        } else {
            Ok(None)
        }
    }

    fn alloc_cwsr(
        &mut self,
        gfx_version: u32,
        is_compute: bool,
    ) -> Result<(Option<Allocation>, Option<cwsr::CwsrSizes>), i32> {
        if gfx_version >= 80000
            && is_compute
            && let Some(sizes) = cwsr::calculate_sizes(self.node_props)
        {
            // CWSR prefers GTT (Host Allocated) so CPU can write the header easily.
            let alloc = self
                .mem_mgr
                .allocate_gpu_memory(
                    self.device,
                    sizes.total_mem_alloc_size as usize,
                    4096,
                    false,
                    false,
                    self.drm_fd,
                )
                .inspect_err(|_e| {
                    eprintln!("Failed to allocate CWSR");
                })?;

            // Initialize Header
            unsafe {
                cwsr::init_header(
                    alloc.ptr,
                    &sizes,
                    self.node_props.num_xcc,
                    0, // ErrorEventId (placeholder)
                    0, // ErrorReason (placeholder)
                );
            }

            return Ok((Some(alloc), Some(sizes)));
        }
        Ok((None, None))
    }

    fn alloc_pointers(&mut self) -> Result<Allocation, i32> {
        let ptr_alloc = self
            .mem_mgr
            .allocate_gpu_memory(self.device, 4096, 4096, false, true, self.drm_fd)
            .inspect_err(|_e| {
                eprintln!("Failed to allocate queue pointers");
            })?;

        unsafe {
            ptr::write_bytes(ptr_alloc.ptr, 0, 4096);
        }
        Ok(ptr_alloc)
    }

    /// Determines EOP buffer size based on ASIC generation.
    const fn calculate_eop_size(gfx_version: u32, is_compute: bool) -> usize {
        let major = (gfx_version / 10000) % 100;
        let minor = (gfx_version / 100) % 100;

        // GFX943 (Aqua Vanjaram) special case
        if major == 9 && minor == 4 {
            return if is_compute { 4096 } else { 0 };
        }

        // GFX8+ (Volcanic Islands and later)
        if major >= 8 {
            return 4096;
        }

        0
    }

    /// Calculates priority integer
    const fn map_priority(p: QueuePriority) -> u32 {
        match p {
            QueuePriority::Minimum => 0,
            QueuePriority::Low => 3,
            QueuePriority::BelowNormal => 5,
            QueuePriority::Normal => 7,
            QueuePriority::AboveNormal => 9,
            QueuePriority::High => 11,
            QueuePriority::Maximum => 15,
        }
    }

    /// Maps the doorbell to CPU accessible memory.
    fn resolve_doorbell_ptr(
        &mut self,
        kernel_offset: u64,
        gfx_version: u32,
    ) -> Result<*mut u32, i32> {
        let is_soc15 = gfx_version >= 90000;

        // Doorbell page size logic: SOC15+ uses 8 byte doorbells (conceptually), pre-SOC15 4KB.
        let doorbell_page_size = if gfx_version >= 90000 { 8 } else { 4 } * 1024;

        let mask = (doorbell_page_size - 1) as u64;

        let mmap_offset = if is_soc15 {
            kernel_offset & !mask
        } else {
            kernel_offset
        };

        let ptr_offset = if is_soc15 { kernel_offset & mask } else { 0 };

        let base_ptr = self.mem_mgr.map_doorbell(
            self.device,
            self.node_id,
            self.node_props.kfd_gpu_id,
            mmap_offset,
            doorbell_page_size as u64,
        )?;

        // Safety: The cast from *mut u8 to *mut u32 is guarded by the hardware logic.
        // The Kernel ensures `kernel_offset` corresponds to a valid 4-byte aligned doorbell register.
        #[allow(clippy::cast_ptr_alignment)]
        unsafe {
            // base_ptr is the start of the page. Add the offset to get the specific queue doorbell.
            let byte_ptr = base_ptr.cast::<u8>().add(ptr_offset as usize);
            Ok(byte_ptr.cast::<u32>())
        }
    }
}
