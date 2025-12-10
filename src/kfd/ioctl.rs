use crate::utils::{ior, iow, iowr};

// ===============================================================================================
// Constants and Versioning
// ===============================================================================================

pub const KFD_IOCTL_BASE: u32 = 0x4B; // 'K'
pub const KFD_IOCTL_MAJOR_VERSION: u32 = 1;
pub const KFD_IOCTL_MINOR_VERSION: u32 = 18;

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct GetVersionArgs {
    pub major_version: u32,
    pub minor_version: u32,
}

// ===============================================================================================
// Queue Management
// ===============================================================================================

pub const KFD_IOC_QUEUE_TYPE_COMPUTE: u32 = 0x0;
pub const KFD_IOC_QUEUE_TYPE_SDMA: u32 = 0x1;
pub const KFD_IOC_QUEUE_TYPE_COMPUTE_AQL: u32 = 0x2;
pub const KFD_IOC_QUEUE_TYPE_SDMA_XGMI: u32 = 0x3;
pub const KFD_IOC_QUEUE_TYPE_SDMA_BY_ENG_ID: u32 = 0x4;

pub const KFD_MAX_QUEUE_PERCENTAGE: u32 = 100;
pub const KFD_MAX_QUEUE_PRIORITY: u32 = 15;

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct CreateQueueArgs {
    pub ring_base_address: u64,
    pub write_pointer_address: u64,
    pub read_pointer_address: u64,
    pub doorbell_offset: u64,

    pub ring_size: u32,
    pub gpu_id: u32,
    pub queue_type: u32,
    pub queue_percentage: u32,
    pub queue_priority: u32,
    pub queue_id: u32,

    pub eop_buffer_address: u64,
    pub eop_buffer_size: u64,
    pub ctx_save_restore_address: u64,
    pub ctx_save_restore_size: u32,
    pub ctl_stack_size: u32,
    pub sdma_engine_id: u32,
    pub pad: u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct DestroyQueueArgs {
    pub queue_id: u32,
    pub pad: u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct UpdateQueueArgs {
    pub ring_base_address: u64,
    pub queue_id: u32,
    pub ring_size: u32,
    pub queue_percentage: u32,
    pub queue_priority: u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct SetCuMaskArgs {
    pub queue_id: u32,
    pub num_cu_mask: u32,
    pub cu_mask_ptr: u64,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct GetQueueWaveStateArgs {
    pub ctl_stack_address: u64,
    pub ctl_stack_used_size: u32,
    pub save_area_used_size: u32,
    pub queue_id: u32,
    pub pad: u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct QueueSnapshotEntry {
    pub exception_status: u64,
    pub ring_base_address: u64,
    pub write_pointer_address: u64,
    pub read_pointer_address: u64,
    pub ctx_save_restore_address: u64,
    pub queue_id: u32,
    pub gpu_id: u32,
    pub ring_size: u32,
    pub queue_type: u32,
    pub ctx_save_restore_area_size: u32,
    pub reserved: u32,
}

// ===============================================================================================
// System Properties & Topology
// ===============================================================================================

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct DbgDeviceInfoEntry {
    pub exception_status: u64,
    pub lds_base: u64,
    pub lds_limit: u64,
    pub scratch_base: u64,
    pub scratch_limit: u64,
    pub gpuvm_base: u64,
    pub gpuvm_limit: u64,
    pub gpu_id: u32,
    pub location_id: u32,
    pub vendor_id: u32,
    pub device_id: u32,
    pub revision_id: u32,
    pub subsystem_vendor_id: u32,
    pub subsystem_device_id: u32,
    pub fw_version: u32,
    pub gfx_target_version: u32,
    pub simd_count: u32,
    pub max_waves_per_simd: u32,
    pub array_count: u32,
    pub simd_arrays_per_engine: u32,
    pub num_xcc: u32,
    pub capability: u32,
    pub debug_prop: u32,
}

// ===============================================================================================
// Memory Policy
// ===============================================================================================

pub const KFD_IOC_CACHE_POLICY_COHERENT: u32 = 0;
pub const KFD_IOC_CACHE_POLICY_NONCOHERENT: u32 = 1;

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct SetMemoryPolicyArgs {
    pub alternate_aperture_base: u64,
    pub alternate_aperture_size: u64,
    pub gpu_id: u32,
    pub default_policy: u32,
    pub alternate_policy: u32,
    pub misc_process_flag: u32,
}

// ===============================================================================================
// Profiling
// ===============================================================================================

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct GetClockCountersArgs {
    pub gpu_clock_counter: u64,
    pub cpu_clock_counter: u64,
    pub system_clock_counter: u64,
    pub system_clock_freq: u64,
    pub gpu_id: u32,
    pub pad: u32,
}

// ===============================================================================================
// Process Apertures
// ===============================================================================================

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct ProcessDeviceApertures {
    pub lds_base: u64,
    pub lds_limit: u64,
    pub scratch_base: u64,
    pub scratch_limit: u64,
    pub gpuvm_base: u64,
    pub gpuvm_limit: u64,
    pub gpu_id: u32,
    pub pad: u32,
}

pub const NUM_OF_SUPPORTED_GPUS: usize = 7;

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct GetProcessAperturesArgs {
    pub process_apertures: [ProcessDeviceApertures; NUM_OF_SUPPORTED_GPUS],
    pub num_of_nodes: u32,
    pub pad: u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct GetProcessAperturesNewArgs {
    pub kfd_process_device_apertures_ptr: u64,
    pub num_of_nodes: u32,
    pub pad: u32,
}

// ===============================================================================================
// Debugger (Deprecated & New)
// ===============================================================================================

// Deprecated debug structs
#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct DbgRegisterArgs {
    pub gpu_id: u32,
    pub pad: u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct DbgUnregisterArgs {
    pub gpu_id: u32,
    pub pad: u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct DbgAddressWatchArgs {
    pub content_ptr: u64,
    pub gpu_id: u32,
    pub buf_size_in_bytes: u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct DbgWaveControlArgs {
    pub content_ptr: u64,
    pub gpu_id: u32,
    pub buf_size_in_bytes: u32,
}

// New Debugger API Constants
pub const KFD_DBG_QUEUE_ERROR_BIT: u32 = 30;
pub const KFD_DBG_QUEUE_INVALID_BIT: u32 = 31;
pub const KFD_DBG_QUEUE_ERROR_MASK: u32 = 1 << KFD_DBG_QUEUE_ERROR_BIT;
pub const KFD_DBG_QUEUE_INVALID_MASK: u32 = 1 << KFD_DBG_QUEUE_INVALID_BIT;

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct RuntimeInfo {
    pub r_debug: u64,
    pub runtime_state: u32,
    pub ttmp_setup: u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct RuntimeEnableArgs {
    pub r_debug: u64,
    pub mode_mask: u32,
    pub capabilities_mask: u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct ContextSaveAreaHeader {
    pub wave_state: ContextSaveAreaHeaderWaveState,
    pub debug_offset: u32,
    pub debug_size: u32,
    pub err_payload_addr: u64,
    pub err_event_id: u32,
    pub reserved1: u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct ContextSaveAreaHeaderWaveState {
    pub control_stack_offset: u32,
    pub control_stack_size: u32,
    pub wave_state_offset: u32,
    pub wave_state_size: u32,
}

// Debug Operations Enums
pub const KFD_IOC_DBG_TRAP_ENABLE: u32 = 0;
pub const KFD_IOC_DBG_TRAP_DISABLE: u32 = 1;
pub const KFD_IOC_DBG_TRAP_SEND_RUNTIME_EVENT: u32 = 2;
pub const KFD_IOC_DBG_TRAP_SET_EXCEPTIONS_ENABLED: u32 = 3;
pub const KFD_IOC_DBG_TRAP_SET_WAVE_LAUNCH_OVERRIDE: u32 = 4;
pub const KFD_IOC_DBG_TRAP_SET_WAVE_LAUNCH_MODE: u32 = 5;
pub const KFD_IOC_DBG_TRAP_SUSPEND_QUEUES: u32 = 6;
pub const KFD_IOC_DBG_TRAP_RESUME_QUEUES: u32 = 7;
pub const KFD_IOC_DBG_TRAP_SET_NODE_ADDRESS_WATCH: u32 = 8;
pub const KFD_IOC_DBG_TRAP_CLEAR_NODE_ADDRESS_WATCH: u32 = 9;
pub const KFD_IOC_DBG_TRAP_SET_FLAGS: u32 = 10;
pub const KFD_IOC_DBG_TRAP_QUERY_DEBUG_EVENT: u32 = 11;
pub const KFD_IOC_DBG_TRAP_QUERY_EXCEPTION_INFO: u32 = 12;
pub const KFD_IOC_DBG_TRAP_GET_QUEUE_SNAPSHOT: u32 = 13;
pub const KFD_IOC_DBG_TRAP_GET_DEVICE_SNAPSHOT: u32 = 14;

// Debug Operation Structs
#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct DbgTrapEnableArgs {
    pub exception_mask: u64,
    pub rinfo_ptr: u64,
    pub rinfo_size: u32,
    pub dbg_fd: u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct DbgTrapSendRuntimeEventArgs {
    pub exception_mask: u64,
    pub gpu_id: u32,
    pub queue_id: u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct DbgTrapSetExceptionsEnabledArgs {
    pub exception_mask: u64,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct DbgTrapSetWaveLaunchOverrideArgs {
    pub override_mode: u32,
    pub enable_mask: u32,
    pub support_request_mask: u32,
    pub pad: u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct DbgTrapSetWaveLaunchModeArgs {
    pub launch_mode: u32,
    pub pad: u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct DbgTrapSuspendQueuesArgs {
    pub exception_mask: u64,
    pub queue_array_ptr: u64,
    pub num_queues: u32,
    pub grace_period: u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct DbgTrapResumeQueuesArgs {
    pub queue_array_ptr: u64,
    pub num_queues: u32,
    pub pad: u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct DbgTrapSetNodeAddressWatchArgs {
    pub address: u64,
    pub mode: u32,
    pub mask: u32,
    pub gpu_id: u32,
    pub id: u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct DbgTrapClearNodeAddressWatchArgs {
    pub gpu_id: u32,
    pub id: u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct DbgTrapSetFlagsArgs {
    pub flags: u32,
    pub pad: u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct DbgTrapQueryDebugEventArgs {
    pub exception_mask: u64,
    pub gpu_id: u32,
    pub queue_id: u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct DbgTrapQueryExceptionInfoArgs {
    pub info_ptr: u64,
    pub info_size: u32,
    pub source_id: u32,
    pub exception_code: u32,
    pub clear_exception: u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct DbgTrapQueueSnapshotArgs {
    pub exception_mask: u64,
    pub snapshot_buf_ptr: u64,
    pub num_queues: u32,
    pub entry_size: u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct DbgTrapDeviceSnapshotArgs {
    pub exception_mask: u64,
    pub snapshot_buf_ptr: u64,
    pub num_devices: u32,
    pub entry_size: u32,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub union DbgTrapArgsUnion {
    pub enable: DbgTrapEnableArgs,
    pub send_runtime_event: DbgTrapSendRuntimeEventArgs,
    pub set_exceptions_enabled: DbgTrapSetExceptionsEnabledArgs,
    pub launch_override: DbgTrapSetWaveLaunchOverrideArgs,
    pub launch_mode: DbgTrapSetWaveLaunchModeArgs,
    pub suspend_queues: DbgTrapSuspendQueuesArgs,
    pub resume_queues: DbgTrapResumeQueuesArgs,
    pub set_node_address_watch: DbgTrapSetNodeAddressWatchArgs,
    pub clear_node_address_watch: DbgTrapClearNodeAddressWatchArgs,
    pub set_flags: DbgTrapSetFlagsArgs,
    pub query_debug_event: DbgTrapQueryDebugEventArgs,
    pub query_exception_info: DbgTrapQueryExceptionInfoArgs,
    pub queue_snapshot: DbgTrapQueueSnapshotArgs,
    pub device_snapshot: DbgTrapDeviceSnapshotArgs,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct DbgTrapArgs {
    pub pid: u32,
    pub op: u32,
    pub data: DbgTrapArgsUnion,
}

// ===============================================================================================
// Events
// ===============================================================================================

pub const KFD_IOC_EVENT_SIGNAL: u32 = 0;
pub const KFD_IOC_EVENT_NODECHANGE: u32 = 1;
pub const KFD_IOC_EVENT_DEVICESTATECHANGE: u32 = 2;
pub const KFD_IOC_EVENT_HW_EXCEPTION: u32 = 3;
pub const KFD_IOC_EVENT_SYSTEM_EVENT: u32 = 4;
pub const KFD_IOC_EVENT_DEBUG_EVENT: u32 = 5;
pub const KFD_IOC_EVENT_PROFILE_EVENT: u32 = 6;
pub const KFD_IOC_EVENT_QUEUE_EVENT: u32 = 7;
pub const KFD_IOC_EVENT_MEMORY: u32 = 8;

pub const KFD_IOC_WAIT_RESULT_COMPLETE: u32 = 0;
pub const KFD_IOC_WAIT_RESULT_TIMEOUT: u32 = 1;
pub const KFD_IOC_WAIT_RESULT_FAIL: u32 = 2;

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct CreateEventArgs {
    pub event_page_offset: u64,
    pub event_trigger_data: u32,
    pub event_type: u32,
    pub auto_reset: u32,
    pub node_id: u32,
    pub event_id: u32,
    pub event_slot_index: u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct DestroyEventArgs {
    pub event_id: u32,
    pub pad: u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct SetEventArgs {
    pub event_id: u32,
    pub pad: u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct ResetEventArgs {
    pub event_id: u32,
    pub pad: u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct MemoryExceptionFailure {
    pub not_present: u32,
    pub read_only: u32,
    pub no_execute: u32,
    pub imprecise: u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct HsaMemoryExceptionData {
    pub failure: MemoryExceptionFailure,
    pub va: u64,
    pub gpu_id: u32,
    pub error_type: u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct HsaHwExceptionData {
    pub reset_type: u32,
    pub reset_cause: u32,
    pub memory_lost: u32,
    pub gpu_id: u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct HsaSignalEventData {
    pub last_event_age: u64,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub union EventDataUnion {
    pub memory_exception_data: HsaMemoryExceptionData,
    pub hw_exception_data: HsaHwExceptionData,
    pub signal_event_data: HsaSignalEventData,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct EventData {
    pub payload: EventDataUnion,
    pub kfd_event_data_ext: u64,
    pub event_id: u32,
    pub pad: u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct WaitEventsArgs {
    pub events_ptr: u64,
    pub num_events: u32,
    pub wait_for_all: u32,
    pub timeout: u32,
    pub wait_result: u32,
}

// ===============================================================================================
// Memory Management (Apertures, VM, Alloc)
// ===============================================================================================

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct SetScratchBackingVaArgs {
    pub va_addr: u64,
    pub gpu_id: u32,
    pub pad: u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct GetTileConfigArgs {
    pub tile_config_ptr: u64,
    pub macro_tile_config_ptr: u64,
    pub num_tile_configs: u32,
    pub num_macro_tile_configs: u32,
    pub gpu_id: u32,
    pub gb_addr_config: u32,
    pub num_banks: u32,
    pub num_ranks: u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct SetTrapHandlerArgs {
    pub tba_addr: u64,
    pub tma_addr: u64,
    pub gpu_id: u32,
    pub pad: u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct AcquireVmArgs {
    pub drm_fd: u32,
    pub gpu_id: u32,
}

// Allocation Flags
pub const KFD_IOC_ALLOC_MEM_FLAGS_VRAM: u32 = 1 << 0;
pub const KFD_IOC_ALLOC_MEM_FLAGS_GTT: u32 = 1 << 1;
pub const KFD_IOC_ALLOC_MEM_FLAGS_USERPTR: u32 = 1 << 2;
pub const KFD_IOC_ALLOC_MEM_FLAGS_DOORBELL: u32 = 1 << 3;
pub const KFD_IOC_ALLOC_MEM_FLAGS_MMIO_REMAP: u32 = 1 << 4;
pub const KFD_IOC_ALLOC_MEM_FLAGS_WRITABLE: u32 = 1 << 31;
pub const KFD_IOC_ALLOC_MEM_FLAGS_EXECUTABLE: u32 = 1 << 30;
pub const KFD_IOC_ALLOC_MEM_FLAGS_PUBLIC: u32 = 1 << 29;
pub const KFD_IOC_ALLOC_MEM_FLAGS_NO_SUBSTITUTE: u32 = 1 << 28;
pub const KFD_IOC_ALLOC_MEM_FLAGS_AQL_QUEUE_MEM: u32 = 1 << 27;
pub const KFD_IOC_ALLOC_MEM_FLAGS_COHERENT: u32 = 1 << 26;
pub const KFD_IOC_ALLOC_MEM_FLAGS_UNCACHED: u32 = 1 << 25;
pub const KFD_IOC_ALLOC_MEM_FLAGS_EXT_COHERENT: u32 = 1 << 24;
pub const KFD_IOC_ALLOC_MEM_FLAGS_CONTIGUOUS_BEST_EFFORT: u32 = 1 << 23;

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct AllocMemoryOfGpuArgs {
    pub va_addr: u64,
    pub size: u64,
    pub handle: u64,
    pub mmap_offset: u64,
    pub gpu_id: u32,
    pub flags: u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct FreeMemoryOfGpuArgs {
    pub handle: u64,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct GetAvailableMemoryArgs {
    pub available: u64,
    pub gpu_id: u32,
    pub pad: u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct MapMemoryToGpuArgs {
    pub handle: u64,
    pub device_ids_array_ptr: u64,
    pub n_devices: u32,
    pub n_success: u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct UnmapMemoryFromGpuArgs {
    pub handle: u64,
    pub device_ids_array_ptr: u64,
    pub n_devices: u32,
    pub n_success: u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct AllocQueueGwsArgs {
    pub queue_id: u32,
    pub num_gws: u32,
    pub first_gws: u32,
    pub pad: u32,
}

// ===============================================================================================
// DMA Buf
// ===============================================================================================

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct GetDmabufInfoArgs {
    pub size: u64,
    pub metadata_ptr: u64,
    pub metadata_size: u32,
    pub gpu_id: u32,
    pub flags: u32,
    pub dmabuf_fd: u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct ImportDmabufArgs {
    pub va_addr: u64,
    pub handle: u64,
    pub gpu_id: u32,
    pub dmabuf_fd: u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct ExportDmabufArgs {
    pub handle: u64,
    pub flags: u32,
    pub dmabuf_fd: u32,
}

// ===============================================================================================
// SMI (System Management Interface)
// ===============================================================================================

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct SmiEventsArgs {
    pub gpu_id: u32,
    pub anon_fd: u32,
}

// ===============================================================================================
// SPM (Streaming Performance Monitor)
// ===============================================================================================

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct SpmArgs {
    pub dest_buf: u64,
    pub buf_size: u32,
    pub op: u32,
    pub timeout: u32,
    pub gpu_id: u32,
    pub bytes_copied: u32,
    pub has_data_loss: u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct SpmBufferHeader {
    pub version: u32,
    pub bytes_copied: u32,
    pub has_data_loss: u32,
    pub reserved: [u32; 5],
}

// ===============================================================================================
// CRIU (Checkpoint Restore In Userspace)
// ===============================================================================================

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct CriuArgs {
    pub devices: u64,
    pub bos: u64,
    pub priv_data: u64,
    pub priv_data_size: u64,
    pub num_devices: u32,
    pub num_bos: u32,
    pub num_objects: u32,
    pub pid: u32,
    pub op: u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct CriuDeviceBucket {
    pub user_gpu_id: u32,
    pub actual_gpu_id: u32,
    pub drm_fd: u32,
    pub pad: u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct CriuBoBucket {
    pub addr: u64,
    pub size: u64,
    pub offset: u64,
    pub restored_offset: u64,
    pub gpu_id: u32,
    pub alloc_flags: u32,
    pub dmabuf_fd: u32,
    pub pad: u32,
}

// ===============================================================================================
// IPC (Inter Process Communication)
// ===============================================================================================

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct IpcExportHandleArgs {
    pub handle: u64,
    pub share_handle: [u32; 4],
    pub gpu_id: u32,
    pub flags: u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct IpcImportHandleArgs {
    pub handle: u64,
    pub va_addr: u64,
    pub mmap_offset: u64,
    pub share_handle: [u32; 4],
    pub gpu_id: u32,
    pub flags: u32,
}

// ===============================================================================================
// Cross Memory Copy
// ===============================================================================================

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct CrossMemoryCopyArgs {
    pub pid: u32,
    pub flags: u32,
    pub src_mem_range_array: u64,
    pub src_mem_array_size: u64,
    pub dst_mem_range_array: u64,
    pub dst_mem_array_size: u64,
    pub bytes_copied: u64,
}

// ===============================================================================================
// SVM (Shared Virtual Memory)
// ===============================================================================================

pub const KFD_IOCTL_SVM_FLAG_HOST_ACCESS: u32 = 0x00000001;
pub const KFD_IOCTL_SVM_FLAG_COHERENT: u32 = 0x00000002;
pub const KFD_IOCTL_SVM_FLAG_HIVE_LOCAL: u32 = 0x00000004;
pub const KFD_IOCTL_SVM_FLAG_GPU_RO: u32 = 0x00000008;
pub const KFD_IOCTL_SVM_FLAG_GPU_EXEC: u32 = 0x00000010;
pub const KFD_IOCTL_SVM_FLAG_GPU_READ_MOSTLY: u32 = 0x00000020;
pub const KFD_IOCTL_SVM_FLAG_GPU_ALWAYS_MAPPED: u32 = 0x00000040;
pub const KFD_IOCTL_SVM_FLAG_EXT_COHERENT: u32 = 0x00000080;

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct SvmAttribute {
    pub type_: u32,
    pub value: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SvmArgs {
    pub start_addr: u64,
    pub size: u64,
    pub op: u32,
    pub nattr: u32,
    // Variable length array attrs[];
    // In Rust FFI, we represent this as a zero-sized array for alignment purposes.
    // Use with caution/unsafe pointer arithmetic.
    pub attrs: [SvmAttribute; 0],
}

// ===============================================================================================
// XNACK
// ===============================================================================================

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct SetXnackModeArgs {
    pub xnack_enabled: i32,
}

// ===============================================================================================
// PC Sampling & Profiler
// ===============================================================================================

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct PcSampleInfo {
    pub interval: u64,
    pub interval_min: u64,
    pub interval_max: u64,
    pub flags: u64,
    pub method: u32,
    pub type_: u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct PcSampleArgs {
    pub sample_info_ptr: u64,
    pub num_sample_info: u32,
    pub op: u32,
    pub gpu_id: u32,
    pub trace_id: u32,
    pub flags: u32,
    pub reserved: u32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct PmcSettings {
    pub gpu_id: u32,
    pub lock: u32,
    pub perfcount_enable: u32,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub union ProfilerArgsUnion {
    pub pc_sample: PcSampleArgs,
    pub pmc: PmcSettings,
    pub version: u32,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct ProfilerArgs {
    pub op: u32,
    pub data: ProfilerArgsUnion,
}

// ===============================================================================================
// AIS (AMD Infinity Storage)
// ===============================================================================================

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct AisInArgs {
    pub handle: u64,
    pub handle_offset: u64,
    pub file_offset: i64,
    pub size: u64,
    pub op: u32,
    pub fd: i32,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct AisOutArgs {
    pub size_copied: u64,
    pub status: i32,
    pub pad: i32,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub union AisArgsUnion {
    pub in_: AisInArgs,
    pub out: AisOutArgs,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct AisArgs {
    pub data: AisArgsUnion,
}

// ===============================================================================================
// IOCTL Command Definitions
// ===============================================================================================

pub const AMDKFD_IOC_GET_VERSION: u32 = ior::<GetVersionArgs>(KFD_IOCTL_BASE, 0x01);
pub const AMDKFD_IOC_CREATE_QUEUE: u32 = iowr::<CreateQueueArgs>(KFD_IOCTL_BASE, 0x02);
pub const AMDKFD_IOC_DESTROY_QUEUE: u32 = iowr::<DestroyQueueArgs>(KFD_IOCTL_BASE, 0x03);
pub const AMDKFD_IOC_SET_MEMORY_POLICY: u32 = iow::<SetMemoryPolicyArgs>(KFD_IOCTL_BASE, 0x04);
pub const AMDKFD_IOC_GET_CLOCK_COUNTERS: u32 = iowr::<GetClockCountersArgs>(KFD_IOCTL_BASE, 0x05);
pub const AMDKFD_IOC_GET_PROCESS_APERTURES: u32 =
    ior::<GetProcessAperturesArgs>(KFD_IOCTL_BASE, 0x06);
pub const AMDKFD_IOC_UPDATE_QUEUE: u32 = iow::<UpdateQueueArgs>(KFD_IOCTL_BASE, 0x07);
pub const AMDKFD_IOC_CREATE_EVENT: u32 = iowr::<CreateEventArgs>(KFD_IOCTL_BASE, 0x08);
pub const AMDKFD_IOC_DESTROY_EVENT: u32 = iow::<DestroyEventArgs>(KFD_IOCTL_BASE, 0x09);
pub const AMDKFD_IOC_SET_EVENT: u32 = iow::<SetEventArgs>(KFD_IOCTL_BASE, 0x0A);
pub const AMDKFD_IOC_RESET_EVENT: u32 = iow::<ResetEventArgs>(KFD_IOCTL_BASE, 0x0B);
pub const AMDKFD_IOC_WAIT_EVENTS: u32 = iowr::<WaitEventsArgs>(KFD_IOCTL_BASE, 0x0C);
pub const AMDKFD_IOC_DBG_REGISTER_DEPRECATED: u32 = iow::<DbgRegisterArgs>(KFD_IOCTL_BASE, 0x0D);
pub const AMDKFD_IOC_DBG_UNREGISTER_DEPRECATED: u32 =
    iow::<DbgUnregisterArgs>(KFD_IOCTL_BASE, 0x0E);
pub const AMDKFD_IOC_DBG_ADDRESS_WATCH_DEPRECATED: u32 =
    iow::<DbgAddressWatchArgs>(KFD_IOCTL_BASE, 0x0F);
pub const AMDKFD_IOC_DBG_WAVE_CONTROL_DEPRECATED: u32 =
    iow::<DbgWaveControlArgs>(KFD_IOCTL_BASE, 0x10);
pub const AMDKFD_IOC_SET_SCRATCH_BACKING_VA: u32 =
    iowr::<SetScratchBackingVaArgs>(KFD_IOCTL_BASE, 0x11);
pub const AMDKFD_IOC_GET_TILE_CONFIG: u32 = iowr::<GetTileConfigArgs>(KFD_IOCTL_BASE, 0x12);
pub const AMDKFD_IOC_SET_TRAP_HANDLER: u32 = iow::<SetTrapHandlerArgs>(KFD_IOCTL_BASE, 0x13);
pub const AMDKFD_IOC_GET_PROCESS_APERTURES_NEW: u32 =
    iowr::<GetProcessAperturesNewArgs>(KFD_IOCTL_BASE, 0x14);
pub const AMDKFD_IOC_ACQUIRE_VM: u32 = iow::<AcquireVmArgs>(KFD_IOCTL_BASE, 0x15);
pub const AMDKFD_IOC_ALLOC_MEMORY_OF_GPU: u32 = iowr::<AllocMemoryOfGpuArgs>(KFD_IOCTL_BASE, 0x16);
pub const AMDKFD_IOC_FREE_MEMORY_OF_GPU: u32 = iow::<FreeMemoryOfGpuArgs>(KFD_IOCTL_BASE, 0x17);
pub const AMDKFD_IOC_MAP_MEMORY_TO_GPU: u32 = iowr::<MapMemoryToGpuArgs>(KFD_IOCTL_BASE, 0x18);
pub const AMDKFD_IOC_UNMAP_MEMORY_FROM_GPU: u32 =
    iowr::<UnmapMemoryFromGpuArgs>(KFD_IOCTL_BASE, 0x19);
pub const AMDKFD_IOC_SET_CU_MASK: u32 = iow::<SetCuMaskArgs>(KFD_IOCTL_BASE, 0x1A);
pub const AMDKFD_IOC_GET_QUEUE_WAVE_STATE: u32 =
    iowr::<GetQueueWaveStateArgs>(KFD_IOCTL_BASE, 0x1B);
pub const AMDKFD_IOC_GET_DMABUF_INFO: u32 = iowr::<GetDmabufInfoArgs>(KFD_IOCTL_BASE, 0x1C);
pub const AMDKFD_IOC_IMPORT_DMABUF: u32 = iowr::<ImportDmabufArgs>(KFD_IOCTL_BASE, 0x1D);
pub const AMDKFD_IOC_ALLOC_QUEUE_GWS: u32 = iowr::<AllocQueueGwsArgs>(KFD_IOCTL_BASE, 0x1E);
pub const AMDKFD_IOC_SMI_EVENTS: u32 = iowr::<SmiEventsArgs>(KFD_IOCTL_BASE, 0x1F);
pub const AMDKFD_IOC_SVM: u32 = iowr::<SvmArgs>(KFD_IOCTL_BASE, 0x20);
pub const AMDKFD_IOC_SET_XNACK_MODE: u32 = iowr::<SetXnackModeArgs>(KFD_IOCTL_BASE, 0x21);
pub const AMDKFD_IOC_CRIU_OP: u32 = iowr::<CriuArgs>(KFD_IOCTL_BASE, 0x22);
pub const AMDKFD_IOC_AVAILABLE_MEMORY: u32 = iowr::<GetAvailableMemoryArgs>(KFD_IOCTL_BASE, 0x23);
pub const AMDKFD_IOC_EXPORT_DMABUF: u32 = iowr::<ExportDmabufArgs>(KFD_IOCTL_BASE, 0x24);
pub const AMDKFD_IOC_RUNTIME_ENABLE: u32 = iowr::<RuntimeEnableArgs>(KFD_IOCTL_BASE, 0x25);
pub const AMDKFD_IOC_DBG_TRAP: u32 = iowr::<DbgTrapArgs>(KFD_IOCTL_BASE, 0x26);

// Extended / Non-upstream IOCTLs
pub const AMDKFD_IOC_IPC_IMPORT_HANDLE: u32 = iowr::<IpcImportHandleArgs>(KFD_IOCTL_BASE, 0x80);
pub const AMDKFD_IOC_IPC_EXPORT_HANDLE: u32 = iowr::<IpcExportHandleArgs>(KFD_IOCTL_BASE, 0x81);
pub const AMDKFD_IOC_CROSS_MEMORY_COPY: u32 = iowr::<CrossMemoryCopyArgs>(KFD_IOCTL_BASE, 0x83);
pub const AMDKFD_IOC_RLC_SPM: u32 = iowr::<SpmArgs>(KFD_IOCTL_BASE, 0x84);
pub const AMDKFD_IOC_PC_SAMPLE: u32 = iowr::<PcSampleArgs>(KFD_IOCTL_BASE, 0x85);
pub const AMDKFD_IOC_PROFILER: u32 = iowr::<ProfilerArgs>(KFD_IOCTL_BASE, 0x86);
pub const AMDKFD_IOC_AIS_OP: u32 = iowr::<AisArgs>(KFD_IOCTL_BASE, 0x87);
