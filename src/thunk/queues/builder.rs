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

#[derive(Debug)]
pub struct QueueResource {
    pub queue_id: u32,
    pub queue_doorbell: u64,            // Virtual address of doorbell
    pub queue_read_ptr: u64,            // Virtual address of read ptr
    pub queue_write_ptr: u64,           // Virtual address of write ptr
    pub queue_err_reason: u64,          // Virtual address of error reason
    pub internal_handle: *mut KmtQueue, // Opaque handle to Thunk's tracking struct
}

/// This holds resources that must persist for the queue's lifetime.
#[repr(C)]
pub struct KmtQueue {
    pub queue_id: u32,
    pub wptr: u64,
    pub rptr: u64,

    // Resource Allocations
    pub eop_mem: Option<Allocation>,
    pub cwsr_mem: Option<Allocation>,
    pub queue_mem: Option<Allocation>,

    // Properties
    pub gfx_version: u32,
    pub cwsr_sizes: Option<cwsr::CwsrSizes>,
    pub use_ats: bool,
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

    pub fn with_type(mut self, t: QueueType) -> Self {
        self.queue_type = t;
        self
    }

    pub fn with_priority(mut self, p: QueuePriority) -> Self {
        self.priority = p;
        self
    }

    pub fn create(mut self) -> Result<QueueResource, i32> {
        // 1. Initialize KmtQueue tracking structure
        let mut q = Box::new(KmtQueue {
            queue_id: 0,
            wptr: 0,
            rptr: 0,
            eop_mem: None,
            cwsr_mem: None,
            queue_mem: None,
            gfx_version: self.node_props.gfx_target_version,
            cwsr_sizes: None,
            use_ats: false, // TODO: Check capability.HSAMMUPresent logic if needed
        });

        // 2. Prepare EOP (End-Of-Pipe) Buffer
        // GFX8 (Tonga): TONGA_PAGE_SIZE
        // GFX8+ (Except Tonga): 4096
        // GFX943 (Aqua Vanjaram): 4096 if Compute, else 0.

        let is_compute = matches!(self.queue_type, QueueType::Compute | QueueType::ComputeAql);
        let eop_size = self.calculate_eop_size(q.gfx_version, is_compute);

        if eop_size > 0 {
            // Pass self.device to the allocator
            let alloc = self
                .mem_mgr
                .allocate_gpu_memory(self.device, eop_size, 4096, true, false, self.drm_fd)
                .map_err(|e| {
                    eprintln!("Failed to allocate EOP");
                    e
                })?;

            unsafe {
                ptr::write_bytes(alloc.ptr, 0, eop_size);
            }
            q.eop_mem = Some(alloc);
        }

        // 3. Prepare CWSR (Context Save/Restore) Area
        // Only for GFX8+ (Carrizo+)
        if q.gfx_version >= 80000 && is_compute {
            if let Some(sizes) = cwsr::calculate_sizes(self.node_props) {
                // "Allocating GTT for CWSR" (Unified memory)
                // It prefers GTT (Host Allocated) for CWSR so CPU can write header.
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
                    .map_err(|e| {
                        eprintln!("Failed to allocate CWSR");
                        e
                    })?;

                // Initialize Header (critical for preventing hangs)
                unsafe {
                    cwsr::init_header(
                        alloc.ptr,
                        &sizes,
                        self.node_props.num_xcc,
                        0, // ErrorEventId (placeholder)
                        0, // ErrorReason (placeholder)
                    );
                }

                q.cwsr_sizes = Some(sizes);
                q.cwsr_mem = Some(alloc);
            }
        }

        // 4. Setup IOCTL Arguments
        let mut args = CreateQueueArgs::default();
        args.gpu_id = self.node_props.kfd_gpu_id;
        args.ring_base_address = self.ring_base;
        args.ring_size = self.ring_size as u32;
        args.queue_type = match self.queue_type {
            QueueType::Compute => KFD_IOC_QUEUE_TYPE_COMPUTE,
            QueueType::Sdma => KFD_IOC_QUEUE_TYPE_SDMA,
            QueueType::ComputeAql => KFD_IOC_QUEUE_TYPE_COMPUTE_AQL,
            QueueType::SdmaXgmi => KFD_IOC_QUEUE_TYPE_SDMA_XGMI,
        };
        args.queue_percentage = self.percentage;
        args.queue_priority = self.map_priority(self.priority);
        args.sdma_engine_id = self.sdma_engine_id;

        // Pointers
        if self.queue_type == QueueType::ComputeAql {
            // For AQL, pointers are inside the ring buffer (handled by CP/Packet Processor).
            // Passing 0 tells KFD to use AQL semantics.
            args.read_pointer_address = 0;
            args.write_pointer_address = 0;
        } else {
            // For PM4, we use the host-allocated rptr/wptr in our KmtQueue struct.
            args.read_pointer_address = &q.rptr as *const _ as u64;
            args.write_pointer_address = &q.wptr as *const _ as u64;
        }

        // EOP & CWSR Args
        if let Some(eop) = &q.eop_mem {
            args.eop_buffer_address = eop.gpu_va;
            args.eop_buffer_size = eop.size as u64;
        }
        if let Some(cwsr) = &q.cwsr_mem {
            args.ctx_save_restore_address = cwsr.gpu_va;
            args.ctx_save_restore_size = q.cwsr_sizes.as_ref().unwrap().ctx_save_restore_size;
            args.ctl_stack_size = q.cwsr_sizes.as_ref().unwrap().ctl_stack_size;
        }

        // 5. Call KFD
        self.device.create_queue(&mut args).map_err(|e| {
            eprintln!("KFD CreateQueue failed: {:?}", e);
            -1
        })?;

        q.queue_id = args.queue_id;

        // Pass self.device to map_doorbell
        let doorbell_ptr = self.resolve_doorbell_ptr(args.doorbell_offset, q.gfx_version)?;

        // 7. Construct Result
        // Leak the box so the KmtQueue persists (referenced by handle).
        // The user must call destroy_queue to drop this box.
        let q_handle = Box::into_raw(q);

        let resource = QueueResource {
            queue_id: args.queue_id,
            queue_doorbell: doorbell_ptr as u64,
            queue_read_ptr: args.read_pointer_address,
            queue_write_ptr: args.write_pointer_address,
            queue_err_reason: 0, // Not implemented yet
            internal_handle: q_handle,
        };

        Ok(resource)
    }

    /// Determines EOP buffer size based on ASIC generation.
    fn calculate_eop_size(&self, gfx_version: u32, is_compute: bool) -> usize {
        let major = (gfx_version / 10000) % 100;
        let minor = (gfx_version / 100) % 100;

        // GFX943 (Aqua Vanjaram)
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
    fn map_priority(&self, p: QueuePriority) -> u32 {
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

    /// Maps the doorbell.
    /// Returns the CPU virtual address of the specific doorbell for this queue.
    fn resolve_doorbell_ptr(
        &mut self,
        kernel_offset: u64,
        gfx_version: u32,
    ) -> Result<*mut u32, i32> {
        let is_soc15 = gfx_version >= 90000;

        let doorbell_page_size = if gfx_version >= 90000 { 8 } else { 4 } * 1024; // Doorbell page size logic

        let mut mmap_offset = kernel_offset;
        let mut ptr_offset = 0;

        if is_soc15 {
            // For SOC15, the kernel return value is explicitly structured.
            // But typically, we pass the raw offset to mmap, and the hardware specific logic
            // handles the "doorbell index within page".
            // Assumption: MemoryManager::map_doorbell handles the mmap() call cache.

            // Mask for page alignment (assuming 4K page for doorbells or calculated size)
            // This relies on the MemoryManager to return the *base* of the doorbell page,
            // and we calculate the addition.

            let mask = (doorbell_page_size - 1) as u64;
            mmap_offset = kernel_offset & !mask;
            ptr_offset = kernel_offset & mask;
        }

        let base_ptr = self.mem_mgr.map_doorbell(
            self.device,
            self.node_id,
            self.node_props.kfd_gpu_id,
            mmap_offset,
        )?;

        unsafe {
            // base_ptr is u32*, need to add byte offset
            let byte_ptr = (base_ptr as *mut u8).add(ptr_offset as usize);
            Ok(byte_ptr as *mut u32)
        }
    }
}
