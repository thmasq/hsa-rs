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
use std::os::unix::io::AsRawFd;
use std::ptr;

// Constants from fmm.c
const SVM_RESERVATION_LIMIT: u64 = (1 << 47) - 1; // 47-bit VA limit
const SVM_MIN_BASE: u64 = 0x1000_0000; // Start at 256MB
const SVM_DEFAULT_ALIGN: usize = 4096;
const SVM_GUARD_PAGES: usize = 1;

/// Flags controlling memory allocation behavior (Maps to HsaMemFlags)
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
    /// Replicates logic from `hsakmt_fmm_init_process_apertures` in fmm.c
    pub fn new(device: &KfdDevice, nodes: &[HsaNodeProperties]) -> Result<Self, i32> {
        // 1. Build Node->GPU mapping
        let mut node_to_gpu_id = HashMap::new();
        let mut gpu_nodes = Vec::new();

        for (idx, node) in nodes.iter().enumerate() {
            if node.kfd_gpu_id != 0 {
                node_to_gpu_id.insert(idx as u32, node.kfd_gpu_id);
                gpu_nodes.push(idx as u32);
            }
        }

        // 2. Query Process Apertures from KFD
        // We need to allocate space for the kernel to fill.
        // We use the count of ALL nodes (CPU+GPU) as `num_of_nodes` expected by KFD.
        let num_sysfs_nodes = nodes.len() as u32;
        let mut apertures_vec = vec![ProcessDeviceApertures::default(); num_sysfs_nodes as usize];

        let mut args = GetProcessAperturesNewArgs {
            kfd_process_device_apertures_ptr: apertures_vec.as_mut_ptr() as u64,
            num_of_nodes: num_sysfs_nodes,
            pad: 0,
        };

        if let Err(e) = device.get_process_apertures_new(&mut args) {
            eprintln!("Failed to get process apertures: {:?}", e);
            return Err(-1);
        }

        // 3. Process Aperture Info
        let mut gpu_apertures = HashMap::new();
        let mut max_gpuvm_limit = 0;

        // Iterate through returned apertures
        // Note: The array index in `apertures_vec` might not map 1:1 to NodeID if KFD logic differs,
        // but typically `GetProcessApertures` returns dense array matching sysfs node order.
        for (i, aperture_info) in apertures_vec.iter().enumerate() {
            if aperture_info.gpu_id == 0 {
                continue; // Skip CPU nodes
            }

            // Find the node_id that corresponds to this gpu_id
            let node_id = match node_to_gpu_id
                .iter()
                .find(|&(_, &gid)| gid == aperture_info.gpu_id)
            {
                Some((&nid, _)) => nid,
                None => continue,
            };

            // Setup specific apertures
            let lds = Aperture::new(
                aperture_info.lds_base,
                aperture_info.lds_limit,
                4096,
                0, // No guard pages for LDS
            );

            let scratch = Aperture::new(
                aperture_info.scratch_base,
                aperture_info.scratch_limit,
                4096,
                0, // No guard pages for Scratch
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

    /// Primary Allocation Function.
    /// Corresponds to `hsakmt_fmm_allocate_device`.
    pub fn allocate_gpu_memory(
        &mut self,
        device: &KfdDevice,
        size: usize,
        align: usize,
        vram: bool,
        public: bool,
        // Optional parameters usually passed via flags in C
        node_id: u32,
    ) -> Result<Allocation, i32> {
        // Construct standard flags based on params
        let mut flags = AllocFlags::default();
        flags.vram = vram;
        flags.gtt = !vram;
        flags.host_access = public || !vram; // GTT is host accessible by default
        flags.execute_access = true;
        flags.coherent = !vram; // GTT coherent by default
        flags.no_substitute = vram && !public; // Private VRAM shouldn't fallback easily

        self.allocate_memory_flags(device, node_id, size, align, flags)
    }

    /// Full allocation function with explicit flags
    pub fn allocate_memory_flags(
        &mut self,
        device: &KfdDevice,
        node_id: u32,
        size: usize,
        align: usize,
        flags: AllocFlags,
    ) -> Result<Allocation, i32> {
        let size = if size == 0 { 4096 } else { size };

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
        let mut ioc_flags = 0;

        if flags.vram {
            ioc_flags |= KFD_IOC_ALLOC_MEM_FLAGS_VRAM;
            if flags.no_substitute {
                ioc_flags |= KFD_IOC_ALLOC_MEM_FLAGS_NO_SUBSTITUTE;
            }
        }
        if flags.gtt {
            ioc_flags |= KFD_IOC_ALLOC_MEM_FLAGS_GTT;
        }
        if flags.doorbell {
            ioc_flags |= KFD_IOC_ALLOC_MEM_FLAGS_DOORBELL;
        }
        if flags.host_access {
            ioc_flags |= KFD_IOC_ALLOC_MEM_FLAGS_PUBLIC;
        }
        // KFD Logic: WRITABLE is needed unless ReadOnly is explicit.
        if !flags.read_only {
            ioc_flags |= KFD_IOC_ALLOC_MEM_FLAGS_WRITABLE;
        }
        if flags.execute_access {
            ioc_flags |= KFD_IOC_ALLOC_MEM_FLAGS_EXECUTABLE;
        }
        if flags.coherent {
            ioc_flags |= KFD_IOC_ALLOC_MEM_FLAGS_COHERENT;
        }
        if flags.uncached {
            ioc_flags |= KFD_IOC_ALLOC_MEM_FLAGS_UNCACHED;
        }
        if flags.extended_coherent {
            ioc_flags |= KFD_IOC_ALLOC_MEM_FLAGS_EXT_COHERENT;
        }
        if flags.aql_queue_mem {
            ioc_flags |= KFD_IOC_ALLOC_MEM_FLAGS_AQL_QUEUE_MEM;
        }
        if flags.contiguous {
            ioc_flags |= KFD_IOC_ALLOC_MEM_FLAGS_CONTIGUOUS_BEST_EFFORT;
        }

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
            eprintln!("KFD Alloc Failed: {:?}", e);
            self.free_va_from_flags(va_addr, size, &flags, node_id);
            return Err(-1);
        }

        // 5. Map to GPU
        let mut map_args = MapMemoryToGpuArgs {
            handle: args.handle,
            device_ids_array_ptr: &gpu_id as *const _ as u64,
            n_devices: 1,
            n_success: 0,
        };

        if let Err(_) = device.map_memory_to_gpu(&mut map_args) {
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

            unsafe {
                let ret = libc::mmap(
                    va_addr as *mut libc::c_void,
                    size,
                    prot,
                    mmap_flags,
                    device.file.as_raw_fd(),
                    args.mmap_offset as libc::off_t,
                );

                if ret == libc::MAP_FAILED {
                    eprintln!("mmap failed for VA 0x{:x}", va_addr);
                    // Cleanup
                    let mut unmap_args = UnmapMemoryFromGpuArgs {
                        handle: args.handle,
                        device_ids_array_ptr: &gpu_id as *const _ as u64,
                        n_devices: 1,
                        n_success: 0,
                    };
                    device.unmap_memory_from_gpu(&mut unmap_args).ok();
                    device.free_memory_of_gpu(args.handle).ok();
                    self.free_va_from_flags(va_addr, size, &flags, node_id);
                    return Err(-1);
                }
                cpu_ptr = ret as *mut u8;
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

    /// Map a doorbell index to a CPU virtual address.
    /// Used by QueueBuilder.
    pub fn map_doorbell(
        &mut self,
        device: &KfdDevice,
        node_id: u32,
        gpu_id: u32,
        doorbell_offset: u64,
    ) -> Result<*mut u32, i32> {
        let size = 4096; // Doorbells are always one page

        // 1. Allocate VA from Alt Aperture (Uncached/Fine Grain)
        let va_addr = self.svm_alt_aperture.allocate_va(size, 4096).ok_or(-12)?;

        // 2. KFD Alloc (Doorbell)
        // Doorbells are special: they don't allocate VRAM/GTT, they map a hardware BAR.
        let flags = KFD_IOC_ALLOC_MEM_FLAGS_DOORBELL
            | KFD_IOC_ALLOC_MEM_FLAGS_WRITABLE
            | KFD_IOC_ALLOC_MEM_FLAGS_PUBLIC
            | KFD_IOC_ALLOC_MEM_FLAGS_COHERENT
            | KFD_IOC_ALLOC_MEM_FLAGS_NO_SUBSTITUTE;

        let mut args = AllocMemoryOfGpuArgs {
            va_addr,
            size: size as u64,
            handle: 0,
            mmap_offset: doorbell_offset, // Important: Input offset for doorbell creation
            gpu_id,
            flags,
        };

        if let Err(_) = device.alloc_memory_of_gpu(&mut args) {
            self.svm_alt_aperture.free_va(va_addr, size);
            return Err(-1);
        }

        // 3. mmap to CPU
        // We use the mmap_offset returned by KFD (which might differ from input)
        let cpu_ptr;
        unsafe {
            let ret = libc::mmap(
                va_addr as *mut libc::c_void,
                size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED | libc::MAP_FIXED, // Force unified address
                device.file.as_raw_fd(),
                args.mmap_offset as libc::off_t,
            );

            if ret == libc::MAP_FAILED {
                device.free_memory_of_gpu(args.handle).ok();
                self.svm_alt_aperture.free_va(va_addr, size);
                return Err(-1);
            }
            cpu_ptr = ret as *mut u32;
        }

        let alloc = Allocation {
            ptr: cpu_ptr as *mut u8,
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
                    libc::munmap(alloc.ptr as *mut _, alloc.size);
                }
            }

            // 2. Free GPU (KFD unmaps internally from GPU)
            device.free_memory_of_gpu(handle).ok();

            // 3. Free VA
            // Identify which aperture owns this address
            if alloc.gpu_va >= self.svm_aperture.bounds().0
                && alloc.gpu_va < self.svm_aperture.bounds().1
            {
                self.svm_aperture.free_va(alloc.gpu_va, alloc.size);
            } else if alloc.gpu_va >= self.svm_alt_aperture.bounds().0
                && alloc.gpu_va < self.svm_alt_aperture.bounds().1
            {
                self.svm_alt_aperture.free_va(alloc.gpu_va, alloc.size);
            } else {
                // Check GPU specific apertures
                if let Some(gpu_aps) = self.gpu_apertures.get_mut(&alloc.node_id) {
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

impl BuilderMemoryManager for MemoryManager {
    fn allocate_gpu_memory(
        &mut self,
        device: &KfdDevice,
        size: usize,
        align: usize,
        vram: bool,
        public: bool,
    ) -> Result<Allocation, i32> {
        // Forward to the inherent method
        // We assume node_id 0 for simple single-GPU cases, or you can expand QueueBuilder
        // to pass the specific node_id via the trait if needed.
        // Ideally, the QueueBuilder should pass the node_id in the trait method,
        // but for now we default to 0 (First GPU) or find the first GPU.

        // Since MemoryManager holds gpu_apertures mapped by node_id, we need a node_id.
        // For the queue builder usage, it is usually allocating EOP/CWSR for the target GPU.
        // We will infer node_id 0 for now as the trait signature in builder doesn't carry node_id
        // (except for map_doorbell).
        // *Correction*: allocate_memory_flags needs a node_id.
        // Let's use the first available GPU node from our map for now.
        let node_id = *self.node_to_gpu_id.keys().next().unwrap_or(&0);

        self.allocate_gpu_memory(device, size, align, vram, public, node_id)
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
    ) -> Result<*mut u32, i32> {
        self.map_doorbell(device, node_id, gpu_id, doorbell_offset)
    }
}
