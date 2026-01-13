use crate::error::HsaResult;
use crate::kfd::device::KfdDevice;
use crate::thunk::events::{EventManager, HsaEvent, HsaEventDescriptor, HsaEventType, HsaSyncVar};
use crate::thunk::memory::{Allocation, MemoryManager};
use crate::thunk::topology;
use std::mem;
use std::os::fd::RawFd;
use std::ptr;
use std::sync::atomic::{AtomicI64, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub type HsaSignalValue = i64;

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
mod x86_utils {
    use std::arch::asm;
    use std::sync::atomic::{AtomicU8, Ordering};

    /// Checks for MWAITX support (CPUID `Fn8000_0001_ECX`[29])
    pub fn supports_mwaitx() -> bool {
        // 0 = Uninitialized, 1 = Supported, 2 = Not Supported
        static MWAITX_SUPPORT: AtomicU8 = AtomicU8::new(0);

        match MWAITX_SUPPORT.load(Ordering::Relaxed) {
            1 => true,
            2 => false,
            _ => {
                let supported = unsafe {
                    let res = std::arch::x86_64::__cpuid(0x8000_0001);
                    (res.ecx & (1 << 29)) != 0
                };

                MWAITX_SUPPORT.store(if supported { 1 } else { 2 }, Ordering::Relaxed);
                supported
            }
        }
    }

    /// Checks if the CPU supports Invariant TSC (CPUID `Fn8000_0007_EDX`[8]).
    /// Invariant TSC runs at a constant rate in all ACPI P-states, C-states,
    /// and T-states, making it safe for timing.
    pub fn is_tsc_safe() -> bool {
        static TSC_SAFE: AtomicU8 = AtomicU8::new(0);
        match TSC_SAFE.load(Ordering::Relaxed) {
            1 => true,
            2 => false,
            _ => {
                let safe = unsafe {
                    let res = std::arch::x86_64::__cpuid(0x8000_0007);
                    (res.edx & (1 << 8)) != 0
                };
                TSC_SAFE.store(if safe { 1 } else { 2 }, Ordering::Relaxed);
                safe
            }
        }
    }

    #[allow(clippy::inline_always)]
    #[inline(always)]
    pub unsafe fn monitorx(addr: *const i64) {
        // EAX/RAX = linear address, ECX = 0 (extensions), EDX = 0 (hints)
        unsafe {
            asm!(
                "monitorx",
                in("rax") addr,
                in("ecx") 0,
                in("edx") 0,
                options(nostack, preserves_flags)
            );
        }
    }

    /// Enters implementation-dependent optimized state until a store occurs or timer expires.
    #[allow(clippy::inline_always)]
    #[inline(always)]
    pub unsafe fn mwaitx(timeout_cycles: u32) {
        // EAX = 0 (Hint C0), ECX = 2 (Enable Timer), EBX = timeout_cycles
        // LLVM reserves RBX, so we must manually save/restore it and move the input.
        unsafe {
            asm!(
                "push rbx",       // Save LLVM's RBX
                "mov rbx, {0}",   // Move timeout into RBX
                "mwaitx",
                "pop rbx",        // Restore LLVM's RBX
                in(reg) u64::from(timeout_cycles),
                in("rax") 0,      // Hint C0
                in("rcx") 2,      // Enable Timer extension
                options(preserves_flags)
            );
        }
    }

    #[allow(clippy::inline_always)]
    #[inline(always)]
    pub unsafe fn rdtsc() -> u64 {
        unsafe { std::arch::x86_64::_rdtsc() }
    }
}

struct WaitGuard<'a>(&'a Signal);
impl Drop for WaitGuard<'_> {
    fn drop(&mut self) {
        self.0.waiting.fetch_sub(1, Ordering::Relaxed);
    }
}

struct GroupWaitGuard<'a>(&'a [&'a Signal]);
impl Drop for GroupWaitGuard<'_> {
    fn drop(&mut self) {
        for s in self.0 {
            s.waiting.fetch_sub(1, Ordering::Relaxed);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum HsaSignalCondition {
    Eq = 0,
    Ne = 1,
    Lt = 2,
    Gte = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum HsaWaitState {
    Blocked = 0,
    Active = 1,
}

#[allow(clippy::inline_always)]
#[inline(always)]
const fn check_condition(value: i64, condition: HsaSignalCondition, compare_value: i64) -> bool {
    match condition {
        HsaSignalCondition::Eq => value == compare_value,
        HsaSignalCondition::Ne => value != compare_value,
        HsaSignalCondition::Lt => value < compare_value,
        HsaSignalCondition::Gte => value >= compare_value,
    }
}

#[allow(dead_code)]
#[repr(i64)]
#[derive(Debug, Copy, Clone)]
enum AmdSignalKind {
    Invalid = 0,
    User = 1,
    Doorbell = -1,
    LegacyDoorbell = -2,
}

#[repr(C, align(64))]
pub struct AmdSignal {
    pub kind: i64,
    pub value: AtomicI64,
    pub event_mailbox_ptr: u64,
    pub event_id: u32,
    pub reserved1: u32,
    pub start_ts: u64,
    pub end_ts: u64,
    pub queue_ptr: u64,
    pub reserved3: [u32; 2],
}

#[repr(C)]
pub struct SharedSignal {
    pub amd_signal: AmdSignal,
    pub sdma_start_ts: u64,
    pub core_signal: u64,
    pub id: u64,
    pub reserved: [u8; 8],
    pub sdma_end_ts: u64,
    pub reserved2: [u8; 24],
}

const _: () = assert!(std::mem::size_of::<AmdSignal>() == 64);
const _: () = assert!(std::mem::align_of::<AmdSignal>() == 64);
const _: () = assert!(std::mem::size_of::<SharedSignal>() == 128);
const _: () = assert!(mem::offset_of!(SharedSignal, sdma_start_ts) == 64);

/// Manages a pool of `SharedSignal` slots with a growth factor.
#[derive(Debug)]
pub struct SignalPool {
    /// Pointers to available 128-byte `SharedSignal` slots.
    free_list: Vec<(*mut SharedSignal, u64)>,
    /// Underlying GTT allocations.
    block_list: Vec<Allocation>,
    /// Number of signals to allocate in the next block.
    next_block_signals: usize,
}

unsafe impl Send for SignalPool {}
unsafe impl Sync for SignalPool {}

impl Default for SignalPool {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalPool {
    /// Initial number of signals per block (1 physical 4KB page).
    const INITIAL_BLOCK_SIGNALS: usize = 32;
    /// Maximum number of signals per block (128KB allocation).
    const MAX_BLOCK_SIGNALS: usize = 1024;

    #[must_use]
    pub const fn new() -> Self {
        Self {
            free_list: Vec::new(),
            block_list: Vec::new(),
            next_block_signals: Self::INITIAL_BLOCK_SIGNALS,
        }
    }

    /// Allocates a `SharedSignal` slot. Grows the pool if necessary.
    pub fn alloc(
        &mut self,
        device: &KfdDevice,
        mem_manager: &mut MemoryManager,
        node_id: u32,
        drm_fd: RawFd,
    ) -> HsaResult<(*mut SharedSignal, u64)> {
        if self.free_list.is_empty() {
            let num_signals = self.next_block_signals;
            let block_bytes = num_signals * std::mem::size_of::<SharedSignal>();

            let allocation = mem_manager.allocate_gtt(device, block_bytes, node_id, drm_fd)?;
            let base_ptr = allocation.as_mut_ptr().cast::<SharedSignal>();
            let base_gpu_va = allocation.gpu_va;

            for i in 0..num_signals {
                unsafe {
                    let slot_ptr = base_ptr.add(i);
                    // Calculate the specific GPU VA for this slot
                    let slot_gpu_va =
                        base_gpu_va + (i * std::mem::size_of::<SharedSignal>()) as u64;

                    std::ptr::write_bytes(slot_ptr, 0, 1);
                    (*slot_ptr).amd_signal.kind = AmdSignalKind::Invalid as i64;
                    (*slot_ptr).id = 0x71FC_CA6A_3D5D_5276;

                    // Push the tuple (ptr, va) to the free list
                    self.free_list.push((slot_ptr, slot_gpu_va));
                }
            }

            self.block_list.push(allocation);
            self.next_block_signals = (num_signals * 2).min(Self::MAX_BLOCK_SIGNALS);
        }

        // Now pop returns the tuple matching the return type
        Ok(self.free_list.pop().expect("Pool must have free slots"))
    }

    /// Returns a slot to the pool for reuse.
    pub unsafe fn free(&mut self, ptr: *mut SharedSignal, gpu_va: u64) {
        unsafe {
            // Mark kind as invalid so any late GPU/CPU access is recognizable.
            (*ptr).amd_signal.kind = AmdSignalKind::Invalid as i64;
            self.free_list.push((ptr, gpu_va));
        }
    }
}

/// A high-level HSA Signal wrapper.
///
/// This struct manages the lifecycle of a `SharedSignal` block and its associated
/// KFD Event. It provides atomic operations and wait primitives.
#[derive(Debug)]
pub struct Signal {
    /// Pointer to the ABI block.
    /// This points into the memory buffer held by `_allocation`.
    ptr: *mut SharedSignal,

    /// Pointer to base address of a signal
    gpu_base_va: u64,

    /// The backing KFD event used for sleeping waits.
    /// We keep an Arc to share it with wait lists.
    event: Arc<HsaEvent>,

    /// The backing memory allocation pool.
    /// Keeping this alive ensures the `ptr` remains valid and mapped in GTT.
    pool: Arc<Mutex<SignalPool>>,

    /// Tracks how many threads are currently waiting on this signal.
    /// Matches `waiting_` in `signal.h`.
    waiting: AtomicU32,

    /// Tracks the agent associated with an asynchronous copy operation.
    /// Used for resource accounting and identifying the copy path (SDMA vs Blit).
    async_copy_agent: AtomicU64,
}

unsafe impl Send for Signal {}
unsafe impl Sync for Signal {}

impl Signal {
    /// Creates a new Signal with an initial value.
    ///
    /// # Arguments
    /// * `initial_value` - The starting value of the signal.
    /// * `device` - The KFD device (for event creation).
    /// * `event_manager` - The event manager instance.
    /// * `mem_manager` - Memory manager to allocate the `SharedSignal` block.
    /// * `drm_fd` - DRM file descriptor.
    /// * `node_id` - Topology node ID.
    pub fn new(
        initial_value: HsaSignalValue,
        device: &KfdDevice,
        event_manager: &mut EventManager,
        mem_manager: &mut MemoryManager,
        pool: Arc<Mutex<SignalPool>>,
        drm_fd: RawFd,
        node_id: u32,
    ) -> HsaResult<Arc<Self>> {
        Self::create_internal(
            initial_value,
            device,
            event_manager,
            mem_manager,
            pool,
            drm_fd,
            node_id,
            AmdSignalKind::User,
            0,
        )
    }

    /// Creates a new Doorbell Signal specifically mapped for hardware queues.
    ///
    /// # Arguments
    /// * `initial_value` - The starting value.
    /// * `device` - The KFD device.
    /// * `event_manager` - The event manager instance.
    /// * `mem_manager` - Memory manager.
    /// * `pool` - Signal pool.
    /// * `drm_fd` - DRM file descriptor.
    /// * `node_id` - Topology node ID.
    /// * `queue_ptr` - Pointer to the AQL queue this doorbell belongs to.
    /// * `is_legacy` - If true, creates a `LegacyDoorbell` signal; otherwise `Doorbell`.
    pub fn new_doorbell(
        initial_value: HsaSignalValue,
        device: &KfdDevice,
        event_manager: &mut EventManager,
        mem_manager: &mut MemoryManager,
        pool: Arc<Mutex<SignalPool>>,
        drm_fd: RawFd,
        node_id: u32,
        queue_ptr: u64,
        is_legacy: bool,
    ) -> HsaResult<Arc<Self>> {
        let kind = if is_legacy {
            AmdSignalKind::LegacyDoorbell
        } else {
            AmdSignalKind::Doorbell
        };
        Self::create_internal(
            initial_value,
            device,
            event_manager,
            mem_manager,
            pool,
            drm_fd,
            node_id,
            kind,
            queue_ptr,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn create_internal(
        initial_value: HsaSignalValue,
        device: &KfdDevice,
        event_manager: &mut EventManager,
        mem_manager: &mut MemoryManager,
        pool: Arc<Mutex<SignalPool>>,
        drm_fd: RawFd,
        node_id: u32,
        kind: AmdSignalKind,
        queue_ptr: u64,
    ) -> HsaResult<Arc<Self>> {
        let (ptr, gpu_base_va) =
            pool.lock()
                .unwrap()
                .alloc(device, mem_manager, node_id, drm_fd)?;

        let event_desc = HsaEventDescriptor {
            event_type: HsaEventType::Signal,
            node_id,
            sync_var: HsaSyncVar {
                user_data: ptr::null_mut(),
                sync_var_size: 0,
            },
        };

        let event =
            event_manager.create_event(device, mem_manager, drm_fd, &event_desc, true, false)?;
        let event = Arc::new(event);

        let signal = Self {
            ptr,
            event: event.clone(),
            pool,
            waiting: AtomicU32::new(0),
            gpu_base_va,
            async_copy_agent: AtomicU64::new(0),
        };

        let signal_arc = Arc::new(signal);

        unsafe {
            let shared = &mut (*ptr);

            shared.amd_signal.kind = kind as i64;
            shared
                .amd_signal
                .value
                .store(initial_value, Ordering::Relaxed);
            shared.amd_signal.event_id = event.event_id;
            shared.amd_signal.event_mailbox_ptr = event.hw_data2;
            shared.amd_signal.queue_ptr = queue_ptr;

            let signal_stable_ptr = Arc::as_ptr(&signal_arc) as u64;
            shared.core_signal = signal_stable_ptr;
        }

        Ok(signal_arc)
    }

    pub fn value_gpu_address(&self) -> u64 {
        self.gpu_base_va + 8
    }

    pub fn signal_handle_gpu_va(&self) -> u64 {
        self.gpu_base_va
    }

    /// Sets the async copy agent and prepares the signal for profiling.
    /// This corresponds to `Signal::async_copy_agent(core::Agent* agent)` in ROCm.
    pub fn set_async_copy_agent(&self, agent_handle: u64) {
        self.async_copy_agent.store(agent_handle, Ordering::Relaxed);
        unsafe {
            // This allows determining if the copy was performed via SDMA or Blit kernel
            // by checking if these values remain 0 later.
            (*self.ptr).sdma_start_ts = 0;
            (*self.ptr).sdma_end_ts = 0;
        }
    }

    /// Retrieves the async copy agent handle.
    pub fn get_async_copy_agent(&self) -> u64 {
        self.async_copy_agent.load(Ordering::Relaxed)
    }

    /// Internal helper to get the atomic reference.
    #[allow(clippy::inline_always)]
    #[inline(always)]
    fn atomic_val(&self) -> &AtomicI64 {
        unsafe { &(*self.ptr).amd_signal.value }
    }

    #[inline]
    pub fn load_relaxed(&self) -> i64 {
        self.atomic_val().load(Ordering::Relaxed)
    }

    #[inline]
    pub fn load_acquire(&self) -> i64 {
        self.atomic_val().load(Ordering::Acquire)
    }

    #[inline]
    pub fn store_relaxed(&self, value: i64) {
        self.atomic_val().store(value, Ordering::Relaxed);
    }

    #[inline]
    pub fn store_release(
        &self,
        value: i64,
        device: &KfdDevice,
        event_manager: &EventManager,
    ) -> HsaResult<()> {
        self.atomic_val().store(value, Ordering::Release);
        self.notify_event(device, event_manager)
    }

    #[inline]
    pub fn exchange_relaxed(&self, value: i64) -> i64 {
        self.atomic_val().swap(value, Ordering::Relaxed)
    }

    #[inline]
    pub fn exchange_acquire(&self, value: i64) -> i64 {
        self.atomic_val().swap(value, Ordering::Acquire)
    }

    #[inline]
    pub fn exchange_release(
        &self,
        value: i64,
        device: &KfdDevice,
        event_manager: &EventManager,
    ) -> i64 {
        let ret = self.atomic_val().swap(value, Ordering::Release);
        let _ = self.notify_event(device, event_manager);
        ret
    }

    #[inline]
    pub fn exchange_acq_rel(
        &self,
        value: i64,
        device: &KfdDevice,
        event_manager: &EventManager,
    ) -> i64 {
        let ret = self.atomic_val().swap(value, Ordering::AcqRel);
        let _ = self.notify_event(device, event_manager);
        ret
    }

    #[inline]
    pub fn cas_relaxed(&self, expected: i64, value: i64) -> i64 {
        self.atomic_val()
            .compare_exchange(expected, value, Ordering::Relaxed, Ordering::Relaxed)
            .unwrap_or_else(|x| x)
    }

    #[inline]
    pub fn cas_acquire(&self, expected: i64, value: i64) -> i64 {
        self.atomic_val()
            .compare_exchange(expected, value, Ordering::Acquire, Ordering::Acquire)
            .unwrap_or_else(|x| x)
    }

    #[inline]
    pub fn cas_release(
        &self,
        expected: i64,
        value: i64,
        device: &KfdDevice,
        event_manager: &EventManager,
    ) -> i64 {
        let res = self.atomic_val().compare_exchange(
            expected,
            value,
            Ordering::Release,
            Ordering::Relaxed,
        );
        if res.is_ok() {
            let _ = self.notify_event(device, event_manager);
        }
        res.unwrap_or_else(|x| x)
    }

    #[inline]
    pub fn cas_acq_rel(
        &self,
        expected: i64,
        value: i64,
        device: &KfdDevice,
        event_manager: &EventManager,
    ) -> i64 {
        let res = self.atomic_val().compare_exchange(
            expected,
            value,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        if res.is_ok() {
            let _ = self.notify_event(device, event_manager);
        }
        res.unwrap_or_else(|x| x)
    }

    #[inline]
    pub fn add_relaxed(&self, value: i64) {
        self.atomic_val().fetch_add(value, Ordering::Relaxed);
    }

    #[inline]
    pub fn add_acquire(&self, value: i64) {
        self.atomic_val().fetch_add(value, Ordering::Acquire);
    }

    #[inline]
    pub fn add_release(&self, value: i64, device: &KfdDevice, event_manager: &EventManager) {
        self.atomic_val().fetch_add(value, Ordering::Release);
        let _ = self.notify_event(device, event_manager);
    }

    #[inline]
    pub fn add_acq_rel(&self, value: i64, device: &KfdDevice, event_manager: &EventManager) {
        self.atomic_val().fetch_add(value, Ordering::AcqRel);
        let _ = self.notify_event(device, event_manager);
    }

    #[inline]
    pub fn sub_relaxed(&self, value: i64) {
        self.atomic_val().fetch_sub(value, Ordering::Relaxed);
    }

    #[inline]
    pub fn sub_acquire(&self, value: i64) {
        self.atomic_val().fetch_sub(value, Ordering::Acquire);
    }

    #[inline]
    pub fn sub_release(&self, value: i64, device: &KfdDevice, event_manager: &EventManager) {
        self.atomic_val().fetch_sub(value, Ordering::Release);
        let _ = self.notify_event(device, event_manager);
    }

    #[inline]
    pub fn sub_acq_rel(&self, value: i64, device: &KfdDevice, event_manager: &EventManager) {
        self.atomic_val().fetch_sub(value, Ordering::AcqRel);
        let _ = self.notify_event(device, event_manager);
    }

    #[inline]
    pub fn and_relaxed(&self, value: i64) {
        self.atomic_val().fetch_and(value, Ordering::Relaxed);
    }

    #[inline]
    pub fn and_acquire(&self, value: i64) {
        self.atomic_val().fetch_and(value, Ordering::Acquire);
    }

    #[inline]
    pub fn and_release(&self, value: i64, device: &KfdDevice, event_manager: &EventManager) {
        self.atomic_val().fetch_and(value, Ordering::Release);
        let _ = self.notify_event(device, event_manager);
    }

    #[inline]
    pub fn and_acq_rel(&self, value: i64, device: &KfdDevice, event_manager: &EventManager) {
        self.atomic_val().fetch_and(value, Ordering::AcqRel);
        let _ = self.notify_event(device, event_manager);
    }

    #[inline]
    pub fn or_relaxed(&self, value: i64) {
        self.atomic_val().fetch_or(value, Ordering::Relaxed);
    }

    #[inline]
    pub fn or_acquire(&self, value: i64) {
        self.atomic_val().fetch_or(value, Ordering::Acquire);
    }

    #[inline]
    pub fn or_release(&self, value: i64, device: &KfdDevice, event_manager: &EventManager) {
        self.atomic_val().fetch_or(value, Ordering::Release);
        let _ = self.notify_event(device, event_manager);
    }

    #[inline]
    pub fn or_acq_rel(&self, value: i64, device: &KfdDevice, event_manager: &EventManager) {
        self.atomic_val().fetch_or(value, Ordering::AcqRel);
        let _ = self.notify_event(device, event_manager);
    }

    #[inline]
    pub fn xor_relaxed(&self, value: i64) {
        self.atomic_val().fetch_xor(value, Ordering::Relaxed);
    }

    #[inline]
    pub fn xor_acquire(&self, value: i64) {
        self.atomic_val().fetch_xor(value, Ordering::Acquire);
    }

    #[inline]
    pub fn xor_release(&self, value: i64, device: &KfdDevice, event_manager: &EventManager) {
        self.atomic_val().fetch_xor(value, Ordering::Release);
        let _ = self.notify_event(device, event_manager);
    }

    #[inline]
    pub fn xor_acq_rel(&self, value: i64, device: &KfdDevice, event_manager: &EventManager) {
        self.atomic_val().fetch_xor(value, Ordering::AcqRel);
        let _ = self.notify_event(device, event_manager);
    }

    // =====================================================================================
    // Wait Logic (Spin -> Sleep)
    // =====================================================================================

    /// Waits for the signal condition to be met.
    pub fn wait_relaxed(
        &self,
        condition: HsaSignalCondition,
        compare_value: i64,
        timeout_hint_clocks: u64,
        wait_hint: HsaWaitState,
        device: &KfdDevice,
        event_manager: &EventManager,
    ) -> i64 {
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        let use_mwaitx = x86_utils::supports_mwaitx();
        #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
        let use_mwaitx = false;

        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        let use_tsc = x86_utils::is_tsc_safe();
        #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
        let use_tsc = false;

        match (use_mwaitx, use_tsc) {
            (true, true) => self.wait_impl::<true, true>(
                condition,
                compare_value,
                timeout_hint_clocks,
                wait_hint,
                device,
                event_manager,
            ),
            (true, false) => self.wait_impl::<true, false>(
                condition,
                compare_value,
                timeout_hint_clocks,
                wait_hint,
                device,
                event_manager,
            ),
            (false, true) => self.wait_impl::<false, true>(
                condition,
                compare_value,
                timeout_hint_clocks,
                wait_hint,
                device,
                event_manager,
            ),
            (false, false) => self.wait_impl::<false, false>(
                condition,
                compare_value,
                timeout_hint_clocks,
                wait_hint,
                device,
                event_manager,
            ),
        }
    }

    #[allow(clippy::inline_always)]
    #[inline(always)]
    fn wait_impl<const USE_MWAITX: bool, const USE_TSC: bool>(
        &self,
        condition: HsaSignalCondition,
        compare_value: i64,
        timeout_hint_clocks: u64,
        wait_hint: HsaWaitState,
        device: &KfdDevice,
        event_manager: &EventManager,
    ) -> i64 {
        let frequency = topology::acquire_system_properties()
            .map(|props| props.timestamp_frequency)
            .unwrap_or(1_000_000_000);

        let mut tsc_start = 0u64;
        let mut tsc_spin_cycles = 0u64;

        let mut inst_start = Instant::now();
        let mut inst_spin_dur = Duration::ZERO;
        let mut inst_timeout = Duration::ZERO;

        if USE_TSC {
            #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
            unsafe {
                tsc_start = x86_utils::rdtsc();
                tsc_spin_cycles = (200 * frequency) / 1_000_000; // 200 us
            }
        } else {
            inst_start = Instant::now();
            inst_spin_dur = Duration::from_micros(20);
            inst_timeout = if timeout_hint_clocks == u64::MAX {
                Duration::from_secs(31_536_000)
            } else {
                let nanos =
                    (u128::from(timeout_hint_clocks) * 1_000_000_000) / u128::from(frequency);
                Duration::from_nanos(nanos as u64)
            };
        }

        self.waiting.fetch_add(1, Ordering::Relaxed);

        std::sync::atomic::fence(Ordering::SeqCst);

        let _guard = WaitGuard(self);

        loop {
            let val = self.load_relaxed();
            if check_condition(val, condition, compare_value) {
                return val;
            }

            if USE_TSC {
                #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
                unsafe {
                    let now = x86_utils::rdtsc();
                    let elapsed = now.wrapping_sub(tsc_start);

                    if timeout_hint_clocks != u64::MAX && elapsed >= timeout_hint_clocks {
                        return val;
                    }

                    if wait_hint != HsaWaitState::Active && elapsed >= tsc_spin_cycles {
                        let remaining_cycles = if timeout_hint_clocks == u64::MAX {
                            u64::MAX
                        } else {
                            timeout_hint_clocks - elapsed
                        };

                        let wait_ms = if remaining_cycles == u64::MAX {
                            u32::MAX
                        } else {
                            ((u128::from(remaining_cycles) * 1000) / u128::from(frequency))
                                .min(u128::from(u32::MAX)) as u32
                        };

                        let events = vec![self.event.as_ref()];
                        let _ =
                            event_manager.wait_on_multiple_events(device, &events, false, wait_ms);
                        continue;
                    }
                }
            } else {
                let elapsed = inst_start.elapsed();
                if elapsed > inst_timeout {
                    return val;
                }

                if wait_hint != HsaWaitState::Active && elapsed >= inst_spin_dur {
                    let remaining = inst_timeout.checked_sub(elapsed).unwrap();
                    let wait_ms = remaining.as_millis().min(u128::from(u32::MAX)) as u32;

                    let events = vec![self.event.as_ref()];
                    let _ = event_manager.wait_on_multiple_events(device, &events, false, wait_ms);
                    continue;
                }
            }

            if USE_MWAITX {
                #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
                unsafe {
                    x86_utils::monitorx(self.atomic_val().as_ptr());

                    let val_recheck = self.load_relaxed();
                    if check_condition(val_recheck, condition, compare_value) {
                        return val_recheck;
                    }

                    let cycle_timeout = if wait_hint == HsaWaitState::Active {
                        1000
                    } else {
                        60000
                    };
                    x86_utils::mwaitx(cycle_timeout);
                }
            } else {
                std::hint::spin_loop();
            }
        }
    }

    pub fn wait_acquire(
        &self,
        condition: HsaSignalCondition,
        compare_value: i64,
        timeout_hint: u64,
        wait_hint: HsaWaitState,
        device: &KfdDevice,
        event_manager: &EventManager,
    ) -> i64 {
        let val = self.wait_relaxed(
            condition,
            compare_value,
            timeout_hint,
            wait_hint,
            device,
            event_manager,
        );
        std::sync::atomic::fence(Ordering::Acquire);
        val
    }

    /// Helper to trigger the KFD interrupt mechanism (Software Signal).
    fn notify_event(&self, device: &KfdDevice, event_manager: &EventManager) -> HsaResult<()> {
        std::sync::atomic::fence(Ordering::SeqCst);

        if self.waiting.load(Ordering::Relaxed) > 0 {
            event_manager.set_event(device, self.event.as_ref())?;
        }
        Ok(())
    }
}

impl Drop for Signal {
    fn drop(&mut self) {
        unsafe {
            (*self.ptr).core_signal = 0;

            self.pool
                .lock()
                .expect("Poisoned pool")
                .free(self.ptr, self.gpu_base_va)
        };
    }
}

// =========================================================================================
// Signal Group Operations
// =========================================================================================

/// Waits for any one of the provided signals to satisfy its condition.
pub fn wait_any(
    signals: &[&Signal],
    conditions: &[HsaSignalCondition],
    values: &[i64],
    timeout_clocks: u64,
    wait_hint: HsaWaitState,
    device: &KfdDevice,
    event_manager: &EventManager,
) -> usize {
    assert_eq!(signals.len(), conditions.len());
    assert_eq!(signals.len(), values.len());

    if signals.len() == 1 {
        let val = signals[0].wait_relaxed(
            conditions[0],
            values[0],
            timeout_clocks,
            wait_hint,
            device,
            event_manager,
        );
        return usize::from(!check_condition(val, conditions[0], values[0]));
    }

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    let use_tsc = x86_utils::is_tsc_safe();
    #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
    let use_tsc = false;

    if use_tsc {
        wait_any_impl::<true>(
            signals,
            conditions,
            values,
            timeout_clocks,
            wait_hint,
            device,
            event_manager,
        )
    } else {
        wait_any_impl::<false>(
            signals,
            conditions,
            values,
            timeout_clocks,
            wait_hint,
            device,
            event_manager,
        )
    }
}

#[allow(clippy::inline_always)]
#[inline(always)]
fn wait_any_impl<const USE_TSC: bool>(
    signals: &[&Signal],
    conditions: &[HsaSignalCondition],
    values: &[i64],
    timeout_clocks: u64,
    wait_hint: HsaWaitState,
    device: &KfdDevice,
    event_manager: &EventManager,
) -> usize {
    let frequency = topology::acquire_system_properties()
        .map(|props| props.timestamp_frequency)
        .unwrap_or(1_000_000_000);

    let mut tsc_start = 0u64;
    let mut tsc_spin_cycles = 0u64;

    let mut inst_start = Instant::now();
    let mut inst_spin_dur = Duration::ZERO;
    let mut inst_timeout = Duration::ZERO;

    if USE_TSC {
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        unsafe {
            tsc_start = x86_utils::rdtsc();
            tsc_spin_cycles = (200 * frequency) / 1_000_000; // 200us
        }
    } else {
        inst_start = Instant::now();
        inst_spin_dur = Duration::from_micros(200);
        inst_timeout = if timeout_clocks == u64::MAX {
            Duration::from_secs(31_536_000)
        } else {
            let nanos = (u128::from(timeout_clocks) * 1_000_000_000) / u128::from(frequency);
            Duration::from_nanos(nanos as u64)
        };
    }

    for s in signals {
        s.waiting.fetch_add(1, Ordering::Relaxed);
    }

    std::sync::atomic::fence(Ordering::SeqCst);

    let _guard = GroupWaitGuard(signals);

    let mut events_ref: Vec<&HsaEvent> = signals.iter().map(|s| s.event.as_ref()).collect();
    events_ref.sort_by_key(|e| e.event_id);
    events_ref.dedup_by_key(|e| e.event_id);

    loop {
        for (i, signal) in signals.iter().enumerate() {
            let val = signal.load_relaxed();
            if check_condition(val, conditions[i], values[i]) {
                return i;
            }
        }

        if USE_TSC {
            #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
            unsafe {
                let now = x86_utils::rdtsc();
                let elapsed = now.wrapping_sub(tsc_start);

                if timeout_clocks != u64::MAX && elapsed >= timeout_clocks {
                    return signals.len();
                }

                if wait_hint == HsaWaitState::Active || elapsed < tsc_spin_cycles {
                    std::hint::spin_loop();
                    continue;
                }

                let remaining_cycles = if timeout_clocks == u64::MAX {
                    u64::MAX
                } else {
                    timeout_clocks - elapsed
                };

                let wait_ms = if remaining_cycles == u64::MAX {
                    u32::MAX
                } else {
                    ((u128::from(remaining_cycles) * 1000) / u128::from(frequency))
                        .min(u128::from(u32::MAX)) as u32
                };

                let _ = event_manager.wait_on_multiple_events(device, &events_ref, false, wait_ms);
            }
        } else {
            let elapsed = inst_start.elapsed();
            if elapsed > inst_timeout {
                return signals.len();
            }

            if wait_hint == HsaWaitState::Active || elapsed < inst_spin_dur {
                std::hint::spin_loop();
                continue;
            }

            let wait_ms = inst_timeout
                .saturating_sub(elapsed)
                .as_millis()
                .min(u128::from(u32::MAX)) as u32;

            let _ = event_manager.wait_on_multiple_events(device, &events_ref, false, wait_ms);
        }
    }
}
