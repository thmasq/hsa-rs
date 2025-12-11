use crate::kfd::sysfs::HsaNodeProperties;
use std::mem;

const HWREG_SIZE_PER_CU: u32 = 0x1000;
const DEBUGGER_BYTES_PER_WAVE: u32 = 32;
const DEBUGGER_BYTES_ALIGN: u32 = 64;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct HsaUserContextSaveAreaHeader {
    /// Byte offset from start of user context save area to the last saved top
    pub control_stack_offset: u32,
    /// Byte size of the last saved control stack data
    pub control_stack_size: u32,
    /// Byte offset from start of user context save area to the last saved base of wave state
    pub wave_state_offset: u32,
    /// Byte size of the last saved wave state data
    pub wave_state_size: u32,
    /// Byte offset from start of the user context save area to the debug memory
    pub debug_offset: u32,
    /// Byte size of the memory reserved for the debugger
    pub debug_size: u32,
    /// Address of the HSA signal payload for reporting error reason
    pub error_reason: u64,
    /// Event ID used for exception signalling
    pub error_event_id: u32,
    pub reserved1: u32,
}

#[derive(Debug)]
pub struct CwsrSizes {
    pub ctl_stack_size: u32,
    pub wg_data_size: u32,
    pub debug_memory_size: u32,
    pub ctx_save_restore_size: u32,
    pub total_mem_alloc_size: u32,
}

impl Default for HsaUserContextSaveAreaHeader {
    fn default() -> Self {
        Self::new()
    }
}

impl HsaUserContextSaveAreaHeader {
    #[must_use]
    pub const fn new() -> Self {
        // Initialize with zeros; callers will populate specific fields
        // Note: offsets/sizes are 0 here and filled by `init_header`
        Self {
            control_stack_offset: 0,
            control_stack_size: 0,
            wave_state_offset: 0,
            wave_state_size: 0,
            debug_offset: 0,
            debug_size: 0,
            error_reason: 0,
            error_event_id: 0,
            reserved1: 0,
        }
    }
}

const fn align_up(val: u32, align: u32) -> u32 {
    (val + align - 1) & !(align - 1)
}

/// Gets VGPR size per CU based on GFX version
const fn get_vgpr_size_per_cu(gfx_version: u32) -> u32 {
    let major = (gfx_version / 10000) % 100;
    let minor = (gfx_version / 100) % 100;
    let step = gfx_version % 100;

    // GFX_VERSION_ALDEBARAN (9.4.2)
    // GFX_VERSION_ARCTURUS  (9.0.8)
    // GFX_VERSION_AQUA_VANJARAM (9.4.3 is common, logic checks & ~(0xff))
    // GFX_VERSION_GFX950    (9.5.0)
    #[rustfmt::skip]
    let is_large_vgpr_gfx9 = major == 9
        && (
            (minor == 0 && step == 8) ||    // Arcturus
            (minor == 4) ||                 // Aldebaran (9.4.2) & Aqua Vanjaram family
            (minor == 5 && step == 0)       // GFX950
        );

    // GFX11+ (Plum Bonito, Wheat Nas, GFX12)
    let is_gfx11_plus = major >= 11;

    if is_large_vgpr_gfx9 {
        0x80000 // 512 KB
    } else if is_gfx11_plus {
        0x60000 // 384 KB
    } else {
        0x40000 // 256 KB (Default for GFX8, GFX9, GFX10)
    }
}

/// Control stack bytes per wave
const fn cntl_stack_bytes_per_wave(gfx_version: u32) -> u32 {
    let major = (gfx_version / 10000) % 100;
    // GFX10+ uses 12 bytes, older use 8
    if major >= 10 { 12 } else { 8 }
}

/// Calculates the required CWSR sizes based on Node Properties.
#[must_use]
pub fn calculate_sizes(props: &HsaNodeProperties) -> Option<CwsrSizes> {
    // Pre-Carrizo/GFX8 not supported in this path
    if props.gfx_target_version < 80000 {
        return None;
    }

    // Safety check for division by zero
    if props.simd_count == 0 || props.simd_per_cu == 0 {
        return None;
    }

    let num_xcc = if props.num_xcc > 0 { props.num_xcc } else { 1 };

    // Total Compute Units per XCC
    let cu_num = props.simd_count / props.simd_per_cu / num_xcc;

    // Calculate Wave Count per XCC
    // Pre-Navi10: MIN(cu_num * 40, num_se / num_sa * 512)
    // Navi10+: cu_num * 32
    let wave_num = if props.gfx_target_version < 100_100 {
        // Pre-Navi10 (10.1.0)
        let max_waves_se = if props.simd_arrays_per_engine > 0 {
            (props.num_shader_banks / props.simd_arrays_per_engine) * 512
        } else {
            u32::MAX
        };
        std::cmp::min(cu_num * 40, max_waves_se)
    } else {
        cu_num * 32
    };

    let ctl_stack_bytes = wave_num * cntl_stack_bytes_per_wave(props.gfx_target_version) + 8;
    #[allow(clippy::cast_possible_truncation)]
    let mut ctl_stack_size = align_up(
        mem::size_of::<HsaUserContextSaveAreaHeader>() as u32 + ctl_stack_bytes,
        4096, // PAGE_SIZE
    );

    // GFX10.1.0 (Navi10) HW bug workaround
    if props.gfx_target_version == 100_100 {
        ctl_stack_size = std::cmp::min(ctl_stack_size, 0x7000);
    }

    let sgpr_size_per_cu = props.sgpr_size_per_cu;
    let lds_size_per_cu = props.lds_size_in_kb * 1024;

    // WG Context Data Size per CU
    // Formula: VGPR + SGPR + LDS + HWREG
    let wg_data_size_per_cu = get_vgpr_size_per_cu(props.gfx_target_version)
        + sgpr_size_per_cu
        + lds_size_per_cu
        + HWREG_SIZE_PER_CU;

    let wg_data_size = cu_num * wg_data_size_per_cu;

    // Debugger memory is allocated at the end of the block
    let debug_memory_size = align_up(wave_num * DEBUGGER_BYTES_PER_WAVE, DEBUGGER_BYTES_ALIGN);

    // Calculate per-XCC size
    let ctx_save_restore_size = ctl_stack_size + align_up(wg_data_size, 4096);

    // Total size includes all XCCs + Debug area
    let total_mem_alloc_size = (ctx_save_restore_size + debug_memory_size) * num_xcc;

    Some(CwsrSizes {
        ctl_stack_size,
        wg_data_size,
        debug_memory_size,
        ctx_save_restore_size,
        total_mem_alloc_size,
    })
}

/// Writes the header into the allocated memory.
///
/// # Safety
/// Caller must ensure `ptr` is valid for `sizes.total_mem_alloc_size` bytes.
/// `ptr` should be the start of the GPU-mapped context save area.
pub unsafe fn init_header(
    ptr: *mut u8,
    sizes: &CwsrSizes,
    num_xcc: u32,
    error_event_id: u32,
    error_reason_ptr: u64,
) {
    let num_xcc = if num_xcc == 0 { 1 } else { num_xcc };

    // The memory layout is: [XCC0 Area] [XCC1 Area] ... [XCCn Area] [Debug Area for all]
    // The `debug_offset` in header `i` points to the *shared* debug area start.

    for i in 0..num_xcc {
        let offset = i * sizes.ctx_save_restore_size;

        let mut header = HsaUserContextSaveAreaHeader::new();

        // Populate fields required by firmware
        header.error_event_id = error_event_id;
        header.error_reason = error_reason_ptr;

        // Calculate offset to the Debug Area.
        // header->DebugOffset = (NumXcc - i) * q->ctx_save_restore_size;
        //
        // Explanation:
        // Current Pos = Base + i * Size
        // Debug Start = Base + NumXcc * Size
        // Offset = Debug Start - Current Pos
        //        = (NumXcc * Size) - (i * Size)
        //        = (NumXcc - i) * Size
        header.debug_offset = (num_xcc - i) * sizes.ctx_save_restore_size;

        // DebugSize is the total debug area size
        header.debug_size = sizes.debug_memory_size * num_xcc;

        // Note: control_stack_offset/size and wave_state_offset/size
        // are updated by the hardware/firmware during a save/restore event.
        // We initialize them to 0.

        // Write to memory
        unsafe {
            ptr.add(offset as usize)
                .cast::<HsaUserContextSaveAreaHeader>()
                .write_unaligned(header);
        }
    }
}
