#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]

use crate::kfd::device::KfdDevice;
use crate::kfd::ioctl::{
    AllocMemoryOfGpuArgs, GetProcessAperturesNewArgs, KFD_IOC_ALLOC_MEM_FLAGS_AQL_QUEUE_MEM,
    KFD_IOC_ALLOC_MEM_FLAGS_COHERENT, KFD_IOC_ALLOC_MEM_FLAGS_CONTIGUOUS_BEST_EFFORT,
    KFD_IOC_ALLOC_MEM_FLAGS_DOORBELL, KFD_IOC_ALLOC_MEM_FLAGS_EXECUTABLE,
    KFD_IOC_ALLOC_MEM_FLAGS_EXT_COHERENT, KFD_IOC_ALLOC_MEM_FLAGS_GTT,
    KFD_IOC_ALLOC_MEM_FLAGS_NO_SUBSTITUTE, KFD_IOC_ALLOC_MEM_FLAGS_PUBLIC,
    KFD_IOC_ALLOC_MEM_FLAGS_UNCACHED, KFD_IOC_ALLOC_MEM_FLAGS_VRAM,
    KFD_IOC_ALLOC_MEM_FLAGS_WRITABLE, MapMemoryToGpuArgs, ProcessDeviceApertures,
    UnmapMemoryFromGpuArgs,
};
use crate::kfd::sysfs::HsaNodeProperties;
use crate::thunk::memory::aperture::Aperture;
use crate::thunk::memory::{Allocation, ApertureAllocator};
use crate::thunk::queues::builder::MemoryManager as BuilderMemoryManager;
use std::collections::HashMap;
use std::os::fd::RawFd;
use std::os::unix::io::AsRawFd;
use std::ptr;

// Constants from fmm.c
const SVM_RESERVATION_LIMIT: u64 = (1 << 47) - 1; // 47-bit VA limit
const SVM_MIN_BASE: u64 = 0x1000_0000; // Start at 256MB
const SVM_DEFAULT_ALIGN: usize = 4096;
const SVM_GUARD_PAGES: usize = 1;

/// Flags controlling memory allocation behavior (Maps to `HsaMemFlags`)
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, Default)]
pub struct AllocFlags {
    pub vram: bool,
    pub gtt: bool,
    pub doorbell: bool,
    pub host_access: bool,
    pub read_only: bool,
    pub execute_access: bool,
    pub coherent: bool,
    pub uncached: bool,
    pub aql_queue_mem: bool,
    pub no_substitute: bool,
    pub contiguous: bool,
    pub extended_coherent: bool,
    pub scratch: bool,
    pub lds: bool,
}

impl AllocFlags {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub const fn vram(mut self) -> Self {
        self.vram = true;
        self
    }

    #[must_use]
    pub const fn gtt(mut self) -> Self {
        self.gtt = true;
        self.host_access = true;
        self.coherent = true;
        self
    }

    #[must_use]
    pub const fn doorbell(mut self) -> Self {
        self.doorbell = true;
        self
    }

    #[must_use]
    pub const fn host_access(mut self) -> Self {
        self.host_access = true;
        self
    }

    #[must_use]
    pub const fn read_only(mut self) -> Self {
        self.read_only = true;
        self
    }

    #[must_use]
    pub const fn executable(mut self) -> Self {
        self.execute_access = true;
        self
    }

    #[must_use]
    pub const fn coherent(mut self) -> Self {
        self.coherent = true;
        self
    }

    #[must_use]
    pub const fn uncached(mut self) -> Self {
        self.uncached = true;
        self
    }

    #[must_use]
    pub const fn aql_queue_mem(mut self) -> Self {
        self.aql_queue_mem = true;
        self
    }

    #[must_use]
    pub const fn no_substitute(mut self) -> Self {
        self.no_substitute = true;
        self
    }

    #[must_use]
    pub const fn contiguous(mut self) -> Self {
        self.contiguous = true;
        self
    }

    /// Converts high-level flags into the raw bitmask required by the KFD IOCTL.
    const fn to_kfd_ioctl_flags(self) -> u32 {
        let mut ioc_flags = 0;

        if self.vram {
            ioc_flags |= KFD_IOC_ALLOC_MEM_FLAGS_VRAM;
            if self.no_substitute {
                ioc_flags |= KFD_IOC_ALLOC_MEM_FLAGS_NO_SUBSTITUTE;
            }
        }
        if self.gtt {
            ioc_flags |= KFD_IOC_ALLOC_MEM_FLAGS_GTT;
        }
        if self.doorbell {
            ioc_flags |= KFD_IOC_ALLOC_MEM_FLAGS_DOORBELL;
        }
        if self.host_access {
            ioc_flags |= KFD_IOC_ALLOC_MEM_FLAGS_PUBLIC;
        }
        // KFD Logic: WRITABLE is needed unless ReadOnly is explicit.
        if !self.read_only {
            ioc_flags |= KFD_IOC_ALLOC_MEM_FLAGS_WRITABLE;
        }
        if self.execute_access {
            ioc_flags |= KFD_IOC_ALLOC_MEM_FLAGS_EXECUTABLE;
        }
        if self.coherent {
            ioc_flags |= KFD_IOC_ALLOC_MEM_FLAGS_COHERENT;
        }
        if self.uncached {
            ioc_flags |= KFD_IOC_ALLOC_MEM_FLAGS_UNCACHED;
        }
        if self.extended_coherent {
            ioc_flags |= KFD_IOC_ALLOC_MEM_FLAGS_EXT_COHERENT;
        }
        if self.aql_queue_mem {
            ioc_flags |= KFD_IOC_ALLOC_MEM_FLAGS_AQL_QUEUE_MEM;
        }
        if self.contiguous {
            ioc_flags |= KFD_IOC_ALLOC_MEM_FLAGS_CONTIGUOUS_BEST_EFFORT;
        }

        ioc_flags
    }
}

/// Per-GPU Apertures derived from KFD Process Info
#[derive(Debug)]
struct GpuApertures {
    lds: Aperture,
    scratch: Aperture,
    gpuvm: Aperture, // Canonical or Non-Canonical GPUVM aperture
}

pub struct MemoryManager {
    // Shared Virtual Memory (SVM) Apertures (System-wide)
    svm_aperture: Aperture,     // Coarse Grain / Default
    svm_alt_aperture: Aperture, // Fine Grain / Uncached

    // Per-node apertures for specific HW requirements
    gpu_apertures: HashMap<u32, GpuApertures>,

    // Mappings
    node_to_gpu_id: HashMap<u32, u32>,
    allocations: HashMap<u64, Allocation>,
}

impl MemoryManager {
    /// Initialize the FMM context by querying KFD for process apertures.
    pub fn new(device: &KfdDevice, nodes: &[HsaNodeProperties]) -> Result<Self, i32> {
        // 1. Build Node->GPU mapping
        let mut node_to_gpu_id = HashMap::new();

        for (idx, node) in nodes.iter().enumerate() {
            if node.kfd_gpu_id != 0 {
                node_to_gpu_id.insert(idx as u32, node.kfd_gpu_id);
            }
        }

        // 2. Query Process Apertures from KFD
        let num_sysfs_nodes = nodes.len() as u32;
        let mut apertures_vec = vec![ProcessDeviceApertures::default(); num_sysfs_nodes as usize];

        let mut args = GetProcessAperturesNewArgs {
            kfd_process_device_apertures_ptr: apertures_vec.as_mut_ptr() as u64,
            num_of_nodes: num_sysfs_nodes,
            pad: 0,
        };

        if let Err(e) = device.get_process_apertures_new(&mut args) {
            eprintln!("Failed to get process apertures: {e:?}");
            return Err(-1);
        }

        // 3. Process Aperture Info
        let mut gpu_apertures = HashMap::new();
        let mut max_gpuvm_limit = 0;

        for aperture_info in &apertures_vec {
            if aperture_info.gpu_id == 0 {
                continue;
            }

            // Find the node_id that corresponds to this gpu_id
            let Some((&node_id, _)) = node_to_gpu_id
                .iter()
                .find(|&(_, &gid)| gid == aperture_info.gpu_id)
            else {
                continue;
            };

            let lds = Aperture::new(aperture_info.lds_base, aperture_info.lds_limit, 4096, 0);

            let scratch = Aperture::new(
                aperture_info.scratch_base,
                aperture_info.scratch_limit,
                4096,
                0,
            );

            // GPUVM Aperture (for non-canonical or specific ranges)
            let gpuvm = Aperture::new(
                aperture_info.gpuvm_base,
                aperture_info.gpuvm_limit,
                4096,
                SVM_GUARD_PAGES as u64,
            );

            if aperture_info.gpuvm_limit > max_gpuvm_limit {
                max_gpuvm_limit = aperture_info.gpuvm_limit;
            }

            gpu_apertures.insert(
                node_id,
                GpuApertures {
                    lds,
                    scratch,
                    gpuvm,
                },
            );
        }

        // 4. Initialize SVM Apertures
        // Logic from `init_svm_apertures` in fmm.c:
        // Reserve 0 to 4GB for Fine Grain (Alt)
        // Reserve 4GB to Limit for Coarse Grain (Default)
        // Adjust limit based on GPU capabilities found above.

        let svm_limit = if max_gpuvm_limit > 0 {
            std::cmp::min(max_gpuvm_limit, SVM_RESERVATION_LIMIT)
        } else {
            SVM_RESERVATION_LIMIT
        };

        let alt_base = SVM_MIN_BASE;
        let alt_size = 4 * 1024 * 1024 * 1024; // 4GB for Alt
        let alt_limit = alt_base + alt_size - 1;

        let def_base = alt_limit + 1;
        let def_limit = svm_limit;

        let svm_alt_aperture = Aperture::new(
            alt_base,
            alt_limit,
            SVM_DEFAULT_ALIGN as u64,
            SVM_GUARD_PAGES as u64,
        );
        let svm_aperture = Aperture::new(
            def_base,
            def_limit,
            SVM_DEFAULT_ALIGN as u64,
            SVM_GUARD_PAGES as u64,
        );

        Ok(Self {
            svm_aperture,
            svm_alt_aperture,
            gpu_apertures,
            node_to_gpu_id,
            allocations: HashMap::new(),
        })
    }

    /// Unified Allocation Function.
    ///
    /// This is the primary entry point for memory allocation.
    /// It handles selecting the correct aperture (SVM, Scratch, LDS, etc.) based on flags,
    /// calls the KFD IOCTL, and maps the memory.
    pub fn allocate(
        &mut self,
        device: &KfdDevice,
        size: usize,
        align: usize,
        flags: AllocFlags,
        node_id: Option<u32>,
        drm_fd: RawFd,
    ) -> Result<Allocation, i32> {
        let size = if size == 0 { 4096 } else { size };

        // Default to the first GPU node if none specified
        let node_id = node_id.unwrap_or_else(|| *self.node_to_gpu_id.keys().next().unwrap_or(&0));

        // 1. Select Aperture
        let aperture = if flags.scratch {
            &mut self.gpu_apertures.get_mut(&node_id).ok_or(-1)?.scratch
        } else if flags.lds {
            &mut self.gpu_apertures.get_mut(&node_id).ok_or(-1)?.lds
        } else if flags.coherent || flags.uncached || flags.doorbell {
            // Fine grain / Signals / Doorbells go to Alt aperture
            &mut self.svm_alt_aperture
        } else {
            // Standard Data (Coarse Grain)
            &mut self.svm_aperture
        };

        // 2. Allocate Virtual Address (VA) from Aperture
        let va_addr = aperture.allocate_va(size, align).ok_or(-12 /* ENOMEM */)?;

        // 3. Prepare IOCTL Flags
        let ioc_flags = flags.to_kfd_ioctl_flags();

        // 4. Call KFD Allocate
        let gpu_id = *self.node_to_gpu_id.get(&node_id).unwrap_or(&0);
        let mut args = AllocMemoryOfGpuArgs {
            va_addr,
            size: size as u64,
            handle: 0,
            mmap_offset: 0,
            gpu_id,
            flags: ioc_flags,
        };

        if let Err(e) = device.alloc_memory_of_gpu(&mut args) {
            eprintln!("KFD Alloc Failed: {e:?}");
            self.free_va_from_flags(va_addr, size, &flags, node_id);
            return Err(-1);
        }

        // 5. Map to GPU
        let mut map_args = MapMemoryToGpuArgs {
            handle: args.handle,
            device_ids_array_ptr: &raw const gpu_id as u64,
            n_devices: 1,
            n_success: 0,
        };

        if device.map_memory_to_gpu(&mut map_args).is_err() {
            device.free_memory_of_gpu(args.handle).ok();
            self.free_va_from_flags(va_addr, size, &flags, node_id);
            return Err(-1);
        }

        // 6. Map to CPU (mmap)
        let mut cpu_ptr = ptr::null_mut();

        if flags.host_access || flags.doorbell {
            let prot = if flags.read_only {
                libc::PROT_READ
            } else {
                libc::PROT_READ | libc::PROT_WRITE
            };

            // MAP_FIXED is critical for SVM: It ensures the CPU address matches the VA we reserved.
            let mmap_flags = libc::MAP_SHARED | libc::MAP_FIXED;

            let mmap_fd = if flags.doorbell {
                device.file.as_raw_fd()
            } else {
                drm_fd
            };

            unsafe {
                let ret = libc::mmap(
                    va_addr as *mut libc::c_void,
                    size,
                    prot,
                    mmap_flags,
                    mmap_fd,
                    args.mmap_offset as libc::off_t,
                );

                if ret == libc::MAP_FAILED {
                    // Cleanup
                    let mut unmap_args = UnmapMemoryFromGpuArgs {
                        handle: args.handle,
                        device_ids_array_ptr: &raw const gpu_id as u64,
                        n_devices: 1,
                        n_success: 0,
                    };
                    device.unmap_memory_from_gpu(&mut unmap_args).ok();
                    device.free_memory_of_gpu(args.handle).ok();
                    self.free_va_from_flags(va_addr, size, &flags, node_id);
                    return Err(-1);
                }
                cpu_ptr = ret.cast::<u8>();
            }
        }

        let allocation = Allocation {
            ptr: cpu_ptr,
            size,
            gpu_va: va_addr,
            handle: args.handle,
            is_userptr: false,
            node_id,
        };

        self.allocations.insert(args.handle, allocation.clone());
        Ok(allocation)
    }

    /// Allocates executable memory on the GPU with specific alignment.
    /// Commonly used for loading code objects (ISA).
    pub fn allocate_exec_aligned_memory_gpu(
        &mut self,
        device: &KfdDevice,
        size: usize,
        align: usize,
        node_id: u32,
        drm_fd: RawFd,
    ) -> Result<Allocation, i32> {
        let flags = AllocFlags::new().vram().executable().no_substitute();

        self.allocate(device, size, align, flags, Some(node_id), drm_fd)
    }

    /// Allocates standard VRAM buffer.
    pub fn allocate_vram(
        &mut self,
        device: &KfdDevice,
        size: usize,
        node_id: u32,
        drm_fd: RawFd,
    ) -> Result<Allocation, i32> {
        let flags = AllocFlags::new().vram();
        self.allocate(device, size, 0, flags, Some(node_id), drm_fd)
    }

    /// Allocates system memory (GTT) accessible by GPU.
    pub fn allocate_gtt(
        &mut self,
        device: &KfdDevice,
        size: usize,
        node_id: u32,
        drm_fd: RawFd,
    ) -> Result<Allocation, i32> {
        let flags = AllocFlags::new().gtt();
        self.allocate(device, size, 0, flags, Some(node_id), drm_fd)
    }

    /// Map a doorbell index to a CPU virtual address.
    pub fn map_doorbell(
        &mut self,
        device: &KfdDevice,
        node_id: u32,
        gpu_id: u32,
        doorbell_offset: u64,
        size: u64,
    ) -> Result<*mut u32, i32> {
        // Doorbell allocation is specialized, so we manually construct the flags here
        // or we could use the generic allocate with doorbell flags if we refactor deeper.
        // For now, keeping the optimized path but using the AllocFlags struct.

        let size = size as usize;
        let va_addr = self.svm_alt_aperture.allocate_va(size, 4096).ok_or(-12)?;

        let flags = KFD_IOC_ALLOC_MEM_FLAGS_DOORBELL
            | KFD_IOC_ALLOC_MEM_FLAGS_WRITABLE
            | KFD_IOC_ALLOC_MEM_FLAGS_COHERENT
            | KFD_IOC_ALLOC_MEM_FLAGS_NO_SUBSTITUTE;

        let mut args = AllocMemoryOfGpuArgs {
            va_addr,
            size: size as u64,
            handle: 0,
            mmap_offset: 0,
            gpu_id,
            flags,
        };

        if let Err(e) = device.alloc_memory_of_gpu(&mut args) {
            eprintln!("[ERROR] map_doorbell: KFD Alloc failed: {e:?}");
            self.svm_alt_aperture.free_va(va_addr, size);
            return Err(-1);
        }

        let cpu_ptr;
        unsafe {
            let ret = libc::mmap(
                va_addr as *mut libc::c_void,
                size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED | libc::MAP_FIXED,
                device.file.as_raw_fd(),
                doorbell_offset as libc::off_t,
            );

            if ret == libc::MAP_FAILED {
                device.free_memory_of_gpu(args.handle).ok();
                self.svm_alt_aperture.free_va(va_addr, size);
                return Err(-1);
            }
            cpu_ptr = ret.cast::<u32>();
        }

        let alloc = Allocation {
            ptr: cpu_ptr.cast::<u8>(),
            size,
            gpu_va: va_addr,
            handle: args.handle,
            is_userptr: false,
            node_id,
        };
        self.allocations.insert(args.handle, alloc);

        Ok(cpu_ptr)
    }

    /// Free a previously allocated memory region
    pub fn free_memory(&mut self, device: &KfdDevice, handle: u64) {
        if let Some(alloc) = self.allocations.remove(&handle) {
            // 1. Munmap CPU
            if !alloc.ptr.is_null() {
                unsafe {
                    libc::munmap(alloc.ptr.cast(), alloc.size);
                }
            }

            // 2. Free GPU (KFD unmaps internally from GPU)
            device.free_memory_of_gpu(handle).ok();

            // 3. Free VA
            if alloc.gpu_va >= self.svm_aperture.bounds().0
                && alloc.gpu_va < self.svm_aperture.bounds().1
            {
                self.svm_aperture.free_va(alloc.gpu_va, alloc.size);
            } else if alloc.gpu_va >= self.svm_alt_aperture.bounds().0
                && alloc.gpu_va < self.svm_alt_aperture.bounds().1
            {
                self.svm_alt_aperture.free_va(alloc.gpu_va, alloc.size);
            } else if let Some(gpu_aps) = self.gpu_apertures.get_mut(&alloc.node_id) {
                if alloc.gpu_va >= gpu_aps.scratch.bounds().0
                    && alloc.gpu_va < gpu_aps.scratch.bounds().1
                {
                    gpu_aps.scratch.free_va(alloc.gpu_va, alloc.size);
                } else if alloc.gpu_va >= gpu_aps.lds.bounds().0
                    && alloc.gpu_va < gpu_aps.lds.bounds().1
                {
                    gpu_aps.lds.free_va(alloc.gpu_va, alloc.size);
                }
            }
        }
    }

    fn free_va_from_flags(&mut self, addr: u64, size: usize, flags: &AllocFlags, node_id: u32) {
        if flags.scratch {
            if let Some(g) = self.gpu_apertures.get_mut(&node_id) {
                g.scratch.free_va(addr, size);
            }
        } else if flags.lds {
            if let Some(g) = self.gpu_apertures.get_mut(&node_id) {
                g.lds.free_va(addr, size);
            }
        } else if flags.coherent || flags.uncached || flags.doorbell {
            self.svm_alt_aperture.free_va(addr, size);
        } else {
            self.svm_aperture.free_va(addr, size);
        }
    }
}

// Implement the Trait for QueueBuilder usage, forwarding to the unified alloc
impl BuilderMemoryManager for MemoryManager {
    fn allocate_gpu_memory(
        &mut self,
        device: &KfdDevice,
        size: usize,
        align: usize,
        vram: bool,
        public: bool,
        drm_fd: RawFd,
    ) -> Result<Allocation, i32> {
        let mut flags = AllocFlags::new();
        if vram {
            flags = flags.vram();
            if !public {
                flags = flags.no_substitute();
            }
        } else {
            flags = flags.gtt();
        }
        if public {
            flags = flags.host_access();
        }

        // Queue buffers typically need execute access and coherency
        flags = flags.executable().coherent();

        self.allocate(device, size, align, flags, None, drm_fd)
    }

    fn free_gpu_memory(&mut self, device: &KfdDevice, alloc: &Allocation) {
        self.free_memory(device, alloc.handle);
    }

    fn map_doorbell(
        &mut self,
        device: &KfdDevice,
        node_id: u32,
        gpu_id: u32,
        doorbell_offset: u64,
        size: u64,
    ) -> Result<*mut u32, i32> {
        self.map_doorbell(device, node_id, gpu_id, doorbell_offset, size)
    }
}
