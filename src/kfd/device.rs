use crate::kfd::ioctl::{
    AMDKFD_IOC_ACQUIRE_VM, AMDKFD_IOC_AIS_OP, AMDKFD_IOC_ALLOC_MEMORY_OF_GPU,
    AMDKFD_IOC_ALLOC_QUEUE_GWS, AMDKFD_IOC_AVAILABLE_MEMORY, AMDKFD_IOC_CREATE_EVENT,
    AMDKFD_IOC_CREATE_QUEUE, AMDKFD_IOC_CRIU_OP, AMDKFD_IOC_CROSS_MEMORY_COPY,
    AMDKFD_IOC_DBG_ADDRESS_WATCH_DEPRECATED, AMDKFD_IOC_DBG_REGISTER_DEPRECATED,
    AMDKFD_IOC_DBG_TRAP, AMDKFD_IOC_DBG_UNREGISTER_DEPRECATED,
    AMDKFD_IOC_DBG_WAVE_CONTROL_DEPRECATED, AMDKFD_IOC_DESTROY_EVENT, AMDKFD_IOC_DESTROY_QUEUE,
    AMDKFD_IOC_EXPORT_DMABUF, AMDKFD_IOC_FREE_MEMORY_OF_GPU, AMDKFD_IOC_GET_CLOCK_COUNTERS,
    AMDKFD_IOC_GET_DMABUF_INFO, AMDKFD_IOC_GET_PROCESS_APERTURES,
    AMDKFD_IOC_GET_PROCESS_APERTURES_NEW, AMDKFD_IOC_GET_QUEUE_WAVE_STATE,
    AMDKFD_IOC_GET_TILE_CONFIG, AMDKFD_IOC_GET_VERSION, AMDKFD_IOC_IMPORT_DMABUF,
    AMDKFD_IOC_IPC_EXPORT_HANDLE, AMDKFD_IOC_IPC_IMPORT_HANDLE, AMDKFD_IOC_MAP_MEMORY_TO_GPU,
    AMDKFD_IOC_PC_SAMPLE, AMDKFD_IOC_PROFILER, AMDKFD_IOC_RESET_EVENT, AMDKFD_IOC_RLC_SPM,
    AMDKFD_IOC_RUNTIME_ENABLE, AMDKFD_IOC_SET_CU_MASK, AMDKFD_IOC_SET_EVENT,
    AMDKFD_IOC_SET_MEMORY_POLICY, AMDKFD_IOC_SET_SCRATCH_BACKING_VA, AMDKFD_IOC_SET_TRAP_HANDLER,
    AMDKFD_IOC_SET_XNACK_MODE, AMDKFD_IOC_SMI_EVENTS, AMDKFD_IOC_SVM,
    AMDKFD_IOC_UNMAP_MEMORY_FROM_GPU, AMDKFD_IOC_UPDATE_QUEUE, AMDKFD_IOC_WAIT_EVENTS,
    AcquireVmArgs, AisArgs, AllocMemoryOfGpuArgs, AllocQueueGwsArgs, CreateEventArgs,
    CreateQueueArgs, CriuArgs, CrossMemoryCopyArgs, DbgAddressWatchArgs, DbgRegisterArgs,
    DbgTrapArgs, DbgUnregisterArgs, DbgWaveControlArgs, DestroyEventArgs, DestroyQueueArgs,
    ExportDmabufArgs, FreeMemoryOfGpuArgs, GetAvailableMemoryArgs, GetClockCountersArgs,
    GetDmabufInfoArgs, GetProcessAperturesArgs, GetProcessAperturesNewArgs, GetQueueWaveStateArgs,
    GetTileConfigArgs, GetVersionArgs, ImportDmabufArgs, IpcExportHandleArgs, IpcImportHandleArgs,
    MapMemoryToGpuArgs, PcSampleArgs, ProfilerArgs, ResetEventArgs, RuntimeEnableArgs,
    SetCuMaskArgs, SetEventArgs, SetMemoryPolicyArgs, SetScratchBackingVaArgs, SetTrapHandlerArgs,
    SetXnackModeArgs, SmiEventsArgs, SpmArgs, SvmArgs, UnmapMemoryFromGpuArgs, UpdateQueueArgs,
    WaitEventsArgs,
};
use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::RawFd;
use std::os::unix::io::AsRawFd;
use std::sync::Arc;

/// A handle to the KFD driver character device (`/dev/kfd`).
///
/// This struct provides methods to issue IOCTLs to the kernel driver.
/// It wraps the file descriptor in an `Arc`, so it is cheap to clone and share
/// across objects (like Queues or Events) that need to persist beyond the initial context.
#[derive(Clone, Debug)]
pub struct KfdDevice {
    pub file: Arc<File>,
}

impl KfdDevice {
    /// Opens the KFD driver device.
    ///
    /// # Errors
    /// Returns an error if `/dev/kfd` cannot be opened (e.g., driver not loaded, permissions).
    pub fn open() -> io::Result<Self> {
        let file = OpenOptions::new().read(true).write(true).open("/dev/kfd")?;

        Ok(Self {
            file: Arc::new(file),
        })
    }

    /// Generic unsafe helper to execute an IOCTL.
    ///
    /// # Safety
    /// The caller must ensure that `arg` points to valid memory appropriate for the specific `cmd`.
    unsafe fn ioctl<T>(&self, cmd: u32, arg: &mut T) -> io::Result<()> {
        let ret = unsafe { libc::ioctl(self.file.as_raw_fd(), cmd as _, arg as *mut T) };
        if ret < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    // ===========================================================================================
    // Versioning
    // ===========================================================================================

    /// Get the KFD driver version.
    pub fn get_version(&self) -> io::Result<GetVersionArgs> {
        let mut args = GetVersionArgs::default();
        unsafe {
            self.ioctl(AMDKFD_IOC_GET_VERSION, &mut args)?;
        }
        Ok(args)
    }

    // ===========================================================================================
    // Queue Management
    // ===========================================================================================

    /// Create a queue for a specific GPU.
    ///
    /// The `args` struct must be populated with the Ring Buffer address, size, and type.
    /// On success, `args.queue_id` and `args.doorbell_offset` will be populated by the driver.
    pub fn create_queue(&self, args: &mut CreateQueueArgs) -> io::Result<()> {
        unsafe { self.ioctl(AMDKFD_IOC_CREATE_QUEUE, args) }
    }

    /// Destroy an existing queue.
    pub fn destroy_queue(&self, queue_id: u32) -> io::Result<()> {
        let mut args = DestroyQueueArgs { queue_id, pad: 0 };
        unsafe { self.ioctl(AMDKFD_IOC_DESTROY_QUEUE, &mut args) }
    }

    /// Update an existing queue's priority or percentage.
    pub fn update_queue(&self, args: &mut UpdateQueueArgs) -> io::Result<()> {
        unsafe { self.ioctl(AMDKFD_IOC_UPDATE_QUEUE, args) }
    }

    /// Set the Compute Unit (CU) mask for a specific queue.
    pub fn set_cu_mask(&self, args: &mut SetCuMaskArgs) -> io::Result<()> {
        unsafe { self.ioctl(AMDKFD_IOC_SET_CU_MASK, args) }
    }

    /// Retrieve the execution state of waves in a queue (context save/restore).
    pub fn get_queue_wave_state(&self, args: &mut GetQueueWaveStateArgs) -> io::Result<()> {
        unsafe { self.ioctl(AMDKFD_IOC_GET_QUEUE_WAVE_STATE, args) }
    }

    // ===========================================================================================
    // Memory Management
    // ===========================================================================================

    /// Acquire the VM from the DRM render node.
    ///
    /// This is a critical step to link the KFD process context with the AMDGPU DRM context.
    pub fn acquire_vm(&self, gpu_id: u32, drm_fd: u32) -> io::Result<()> {
        let mut args = AcquireVmArgs { gpu_id, drm_fd };
        unsafe { self.ioctl(AMDKFD_IOC_ACQUIRE_VM, &mut args) }
    }

    /// Set the memory policy (coherency) for a specific GPU or aperture.
    pub fn set_memory_policy(&self, args: &mut SetMemoryPolicyArgs) -> io::Result<()> {
        unsafe { self.ioctl(AMDKFD_IOC_SET_MEMORY_POLICY, args) }
    }

    /// Allocate memory on a specific GPU (VRAM, GTT, Doorbell, etc.).
    ///
    /// On success, `args.handle` will contain the handle to the allocated memory.
    pub fn alloc_memory_of_gpu(&self, args: &mut AllocMemoryOfGpuArgs) -> io::Result<()> {
        unsafe { self.ioctl(AMDKFD_IOC_ALLOC_MEMORY_OF_GPU, args) }
    }

    /// Free memory previously allocated via `alloc_memory_of_gpu`.
    pub fn free_memory_of_gpu(&self, handle: u64) -> io::Result<()> {
        let mut args = FreeMemoryOfGpuArgs { handle };
        unsafe { self.ioctl(AMDKFD_IOC_FREE_MEMORY_OF_GPU, &mut args) }
    }

    /// Map allocated memory to one or more GPUs.
    pub fn map_memory_to_gpu(&self, args: &mut MapMemoryToGpuArgs) -> io::Result<()> {
        unsafe { self.ioctl(AMDKFD_IOC_MAP_MEMORY_TO_GPU, args) }
    }

    /// Unmap memory from GPUs.
    pub fn unmap_memory_from_gpu(&self, args: &mut UnmapMemoryFromGpuArgs) -> io::Result<()> {
        unsafe { self.ioctl(AMDKFD_IOC_UNMAP_MEMORY_FROM_GPU, args) }
    }

    /// Query available memory for a specific GPU.
    pub fn get_available_memory(&self, gpu_id: u32) -> io::Result<u64> {
        let mut args = GetAvailableMemoryArgs {
            available: 0,
            gpu_id,
            pad: 0,
        };
        unsafe {
            self.ioctl(AMDKFD_IOC_AVAILABLE_MEMORY, &mut args)?;
        }
        Ok(args.available)
    }

    /// Set the virtual address for scratch backing memory.
    pub fn set_scratch_backing_va(&self, args: &mut SetScratchBackingVaArgs) -> io::Result<()> {
        unsafe { self.ioctl(AMDKFD_IOC_SET_SCRATCH_BACKING_VA, args) }
    }

    // ===========================================================================================
    // Topology & System Info
    // ===========================================================================================

    /// Retrieve the process apertures (LDS, Scratch, GPUVM limits).
    ///
    /// Prefer using `get_process_apertures_new` for newer hardware support.
    pub fn get_process_apertures(&self, args: &mut GetProcessAperturesArgs) -> io::Result<()> {
        unsafe { self.ioctl(AMDKFD_IOC_GET_PROCESS_APERTURES, args) }
    }

    /// Retrieve process apertures using the new API (supports more nodes).
    ///
    /// `args.kfd_process_device_apertures_ptr` must point to a user-allocated array of `ProcessDeviceApertures`.
    pub fn get_process_apertures_new(
        &self,
        args: &mut GetProcessAperturesNewArgs,
    ) -> io::Result<()> {
        unsafe { self.ioctl(AMDKFD_IOC_GET_PROCESS_APERTURES_NEW, args) }
    }

    /// Retrieve tile configuration for the GPU.
    pub fn get_tile_config(&self, args: &mut GetTileConfigArgs) -> io::Result<()> {
        unsafe { self.ioctl(AMDKFD_IOC_GET_TILE_CONFIG, args) }
    }

    /// Retrieve GPU and System clock counters.
    pub fn get_clock_counters(&self, args: &mut GetClockCountersArgs) -> io::Result<()> {
        unsafe { self.ioctl(AMDKFD_IOC_GET_CLOCK_COUNTERS, args) }
    }

    // ===========================================================================================
    // Events & Synchronization
    // ===========================================================================================

    /// Create an event (signal, memory exception, etc.).
    pub fn create_event(&self, args: &mut CreateEventArgs) -> io::Result<()> {
        unsafe { self.ioctl(AMDKFD_IOC_CREATE_EVENT, args) }
    }

    /// Destroy an event.
    pub fn destroy_event(&self, event_id: u32) -> io::Result<()> {
        let mut args = DestroyEventArgs { event_id, pad: 0 };
        unsafe { self.ioctl(AMDKFD_IOC_DESTROY_EVENT, &mut args) }
    }

    /// Set an event to the signaled state.
    pub fn set_event(&self, event_id: u32) -> io::Result<()> {
        let mut args = SetEventArgs { event_id, pad: 0 };
        unsafe { self.ioctl(AMDKFD_IOC_SET_EVENT, &mut args) }
    }

    /// Reset an event to the unsignaled state.
    pub fn reset_event(&self, event_id: u32) -> io::Result<()> {
        let mut args = ResetEventArgs { event_id, pad: 0 };
        unsafe { self.ioctl(AMDKFD_IOC_RESET_EVENT, &mut args) }
    }

    /// Wait for one or more events to be signaled.
    pub fn wait_events(&self, args: &mut WaitEventsArgs) -> io::Result<()> {
        unsafe { self.ioctl(AMDKFD_IOC_WAIT_EVENTS, args) }
    }

    // ===========================================================================================
    // Trap Handling & Debugging
    // ===========================================================================================

    /// Set the trap handler code address (TBA/TMA) for the GPU.
    pub fn set_trap_handler(&self, args: &mut SetTrapHandlerArgs) -> io::Result<()> {
        unsafe { self.ioctl(AMDKFD_IOC_SET_TRAP_HANDLER, args) }
    }

    /// Perform a debug trap operation.
    ///
    /// This is the primary entry point for the new Debugger API.
    pub fn dbg_trap(&self, args: &mut DbgTrapArgs) -> io::Result<()> {
        unsafe { self.ioctl(AMDKFD_IOC_DBG_TRAP, args) }
    }

    // Deprecated Debug APIs (included for completeness)
    pub fn dbg_register_deprecated(&self, gpu_id: u32) -> io::Result<()> {
        let mut args = DbgRegisterArgs { gpu_id, pad: 0 };
        unsafe { self.ioctl(AMDKFD_IOC_DBG_REGISTER_DEPRECATED, &mut args) }
    }

    pub fn dbg_unregister_deprecated(&self, gpu_id: u32) -> io::Result<()> {
        let mut args = DbgUnregisterArgs { gpu_id, pad: 0 };
        unsafe { self.ioctl(AMDKFD_IOC_DBG_UNREGISTER_DEPRECATED, &mut args) }
    }

    pub fn dbg_address_watch_deprecated(&self, args: &mut DbgAddressWatchArgs) -> io::Result<()> {
        unsafe { self.ioctl(AMDKFD_IOC_DBG_ADDRESS_WATCH_DEPRECATED, args) }
    }

    pub fn dbg_wave_control_deprecated(&self, args: &mut DbgWaveControlArgs) -> io::Result<()> {
        unsafe { self.ioctl(AMDKFD_IOC_DBG_WAVE_CONTROL_DEPRECATED, args) }
    }

    // ===========================================================================================
    // DMA Buffer Interop
    // ===========================================================================================

    /// Get information about an imported DMA buffer.
    pub fn get_dmabuf_info(&self, args: &mut GetDmabufInfoArgs) -> io::Result<()> {
        unsafe { self.ioctl(AMDKFD_IOC_GET_DMABUF_INFO, args) }
    }

    /// Import a DMA buffer into the KFD context.
    pub fn import_dmabuf(&self, args: &mut ImportDmabufArgs) -> io::Result<()> {
        unsafe { self.ioctl(AMDKFD_IOC_IMPORT_DMABUF, args) }
    }

    /// Export a KFD memory allocation as a DMA buffer.
    pub fn export_dmabuf(&self, args: &mut ExportDmabufArgs) -> io::Result<()> {
        unsafe { self.ioctl(AMDKFD_IOC_EXPORT_DMABUF, args) }
    }

    // ===========================================================================================
    // Advanced Features (GWS, SVM, SMI, CRIU, XNACK)
    // ===========================================================================================

    /// Allocate Global Wavefront Switch (GWS) memory for a queue.
    pub fn alloc_queue_gws(&self, args: &mut AllocQueueGwsArgs) -> io::Result<()> {
        unsafe { self.ioctl(AMDKFD_IOC_ALLOC_QUEUE_GWS, args) }
    }

    /// Shared Virtual Memory (SVM) operations.
    ///
    /// This handles Unified Memory attributes, migration, and prefetch.
    /// Note: `args` contains a pointer to an attribute array which must be valid.
    pub fn svm(&self, args: &mut SvmArgs) -> io::Result<()> {
        unsafe { self.ioctl(AMDKFD_IOC_SVM, args) }
    }

    /// Configure XNACK mode (retry on page fault).
    pub fn set_xnack_mode(&self, xnack_enabled: bool) -> io::Result<()> {
        let mut args = SetXnackModeArgs {
            xnack_enabled: if xnack_enabled { 1 } else { 0 },
        };
        unsafe { self.ioctl(AMDKFD_IOC_SET_XNACK_MODE, &mut args) }
    }

    /// System Management Interface (SMI) events.
    pub fn smi_events(&self, args: &mut SmiEventsArgs) -> io::Result<()> {
        unsafe { self.ioctl(AMDKFD_IOC_SMI_EVENTS, args) }
    }

    /// Checkpoint Restore In Userspace (CRIU) operations.
    pub fn criu_op(&self, args: &mut CriuArgs) -> io::Result<()> {
        unsafe { self.ioctl(AMDKFD_IOC_CRIU_OP, args) }
    }

    // ===========================================================================================
    // Non-Upstream / Extended IOCTLs
    // ===========================================================================================

    /// Import an IPC handle.
    pub fn ipc_import_handle(&self, args: &mut IpcImportHandleArgs) -> io::Result<()> {
        unsafe { self.ioctl(AMDKFD_IOC_IPC_IMPORT_HANDLE, args) }
    }

    /// Export an IPC handle.
    pub fn ipc_export_handle(&self, args: &mut IpcExportHandleArgs) -> io::Result<()> {
        unsafe { self.ioctl(AMDKFD_IOC_IPC_EXPORT_HANDLE, args) }
    }

    /// Cross-process memory copy.
    pub fn cross_memory_copy(&self, args: &mut CrossMemoryCopyArgs) -> io::Result<()> {
        unsafe { self.ioctl(AMDKFD_IOC_CROSS_MEMORY_COPY, args) }
    }

    /// Runtime enable (coordinates with debuggers).
    pub fn runtime_enable(&self, args: &mut RuntimeEnableArgs) -> io::Result<()> {
        unsafe { self.ioctl(AMDKFD_IOC_RUNTIME_ENABLE, args) }
    }

    /// Streaming Performance Monitor (SPM).
    pub fn spm(&self, args: &mut SpmArgs) -> io::Result<()> {
        unsafe { self.ioctl(AMDKFD_IOC_RLC_SPM, args) }
    }

    /// PC Sampling.
    pub fn pc_sample(&self, args: &mut PcSampleArgs) -> io::Result<()> {
        unsafe { self.ioctl(AMDKFD_IOC_PC_SAMPLE, args) }
    }

    /// Profiler control.
    pub fn profiler(&self, args: &mut ProfilerArgs) -> io::Result<()> {
        unsafe { self.ioctl(AMDKFD_IOC_PROFILER, args) }
    }

    /// AMD Infinity Storage (AIS) operations.
    pub fn ais_op(&self, args: &mut AisArgs) -> io::Result<()> {
        unsafe { self.ioctl(AMDKFD_IOC_AIS_OP, args) }
    }
}

impl AsRawFd for KfdDevice {
    fn as_raw_fd(&self) -> RawFd {
        self.file.as_raw_fd()
    }
}
