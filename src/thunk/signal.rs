use crate::error::HsaResult;
use crate::kfd::device::KfdDevice;
use crate::thunk::events::{EventManager, HsaEvent, HsaEventDescriptor, HsaEventType, HsaSyncVar};
use crate::thunk::memory::{Allocation, MemoryManager};
use crate::thunk::topology;
use std::mem;
use std::os::fd::RawFd;
use std::ptr;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicU32, Ordering};
use std::time::{Duration, Instant};

pub type HsaSignalValue = i64;

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
mod x86_utils {
    use std::arch::asm;
    use std::sync::atomic::{AtomicU8, Ordering};

    /// Checks for MWAITX support (CPUID Fn8000_0001_ECX[29])
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

    /// Sets up the address range for monitoring.
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
                in(reg) timeout_cycles as u64,
                in("rax") 0,      // Hint C0
                in("rcx") 2,      // Enable Timer extension
                options(preserves_flags)
            );
        }
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

#[inline(always)]
fn check_condition(value: i64, condition: HsaSignalCondition, compare_value: i64) -> bool {
    match condition {
        HsaSignalCondition::Eq => value == compare_value,
        HsaSignalCondition::Ne => value != compare_value,
        HsaSignalCondition::Lt => value < compare_value,
        HsaSignalCondition::Gte => value >= compare_value,
    }
}

#[allow(dead_code)]
#[repr(i64)]
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

/// A high-level HSA Signal wrapper.
///
/// This struct manages the lifecycle of a `SharedSignal` block and its associated
/// KFD Event. It provides atomic operations and wait primitives.
#[derive(Debug)]
pub struct Signal {
    /// Pointer to the ABI block.
    /// This points into the memory buffer held by `_allocation`.
    ptr: *mut SharedSignal,

    /// The backing KFD event used for sleeping waits.
    /// We keep an Arc to share it with wait lists.
    event: Arc<HsaEvent>,

    /// The backing memory allocation.
    /// Keeping this alive ensures the `ptr` remains valid and mapped in GTT.
    _allocation: Allocation,

    /// Tracks how many threads are currently waiting on this signal.
    /// Matches `waiting_` in `signal.h`.
    waiting: AtomicU32,
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
        drm_fd: RawFd,
        node_id: u32,
    ) -> HsaResult<Self> {
        // 1. Allocate memory for SharedSignal (128 bytes).
        // We use allocate_gtt to ensure it's system memory accessible by the GPU (Coherent).
        // The memory manager handles 4KB page granularity, but we only need 128 bytes.
        // In a real optimized runtime, we would use a pool allocator (like `SharedSignalPool_t`).
        // For this implementation, we allocate a full page per signal to ensure correctness and safety.
        let allocation = mem_manager.allocate_gtt(
            device,
            std::mem::size_of::<SharedSignal>(),
            node_id,
            drm_fd,
        )?;

        let ptr = allocation.as_mut_ptr() as *mut SharedSignal;
        unsafe {
            ptr::write_bytes(ptr, 0, 1);
        }

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

        unsafe {
            let signal = &mut (*ptr).amd_signal;
            signal.kind = AmdSignalKind::User as i64;
            signal.value.store(initial_value, Ordering::Relaxed);

            signal.event_id = event.event_id;
            signal.event_mailbox_ptr = event.hw_data2;

            (*ptr).core_signal = 0;
            (*ptr).id = 0x71FC_CA6A_3D5D_5276;
        }

        Ok(Self {
            ptr,
            event,
            _allocation: allocation,
            waiting: AtomicU32::new(0),
        })
    }

    /// Internal helper to get the atomic reference.
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
        self.atomic_val().store(value, Ordering::Relaxed)
    }

    #[inline]
    pub fn store_release(
        &self,
        value: i64,
        device: &KfdDevice,
        event_manager: &EventManager,
    ) -> HsaResult<()> {
        self.atomic_val().store(value, Ordering::Release);
        // A store release acts as a signal. We must notify waiting threads.
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

        let timeout_duration = if timeout_hint_clocks == u64::MAX {
            Duration::from_secs(31536000) // ~1 year (Wait Forever)
        } else {
            let frequency = topology::acquire_system_properties()
                .map(|props| props.timestamp_frequency)
                .unwrap_or(1_000_000_000);

            let nanos = (timeout_hint_clocks as u128 * 1_000_000_000) / frequency as u128;
            Duration::from_nanos(nanos as u64)
        };

        let start = Instant::now();
        let spin_duration = Duration::from_micros(20);

        self.waiting.fetch_add(1, Ordering::Relaxed);

        std::sync::atomic::fence(Ordering::SeqCst);

        let _guard = WaitGuard(self);

        loop {
            let val = self.load_relaxed();
            if check_condition(val, condition, compare_value) {
                return val;
            }

            let elapsed = start.elapsed();
            if elapsed > timeout_duration {
                return val;
            }

            if wait_hint == HsaWaitState::Active || elapsed < spin_duration {
                if use_mwaitx {
                    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
                    {
                        unsafe { x86_utils::monitorx(self.atomic_val().as_ptr()) };

                        let val_recheck = self.load_relaxed();
                        if check_condition(val_recheck, condition, compare_value) {
                            return val_recheck;
                        }

                        let cycle_timeout = if wait_hint == HsaWaitState::Active {
                            1000
                        } else {
                            60000
                        };

                        unsafe { x86_utils::mwaitx(cycle_timeout) };
                        continue;
                    }
                }

                std::hint::spin_loop();
                continue;
            }

            let remaining = timeout_duration - elapsed;
            let wait_ms = remaining.as_millis().min(u32::MAX as u128) as u32;

            let events_to_wait = vec![self.event.as_ref()];

            let _ = event_manager.wait_on_multiple_events(device, &events_to_wait, false, wait_ms);
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
        return if check_condition(val, conditions[0], values[0]) {
            0
        } else {
            1
        };
    }

    let frequency = topology::acquire_system_properties()
        .map(|props| props.timestamp_frequency)
        .unwrap_or(1_000_000_000);

    let timeout_duration = if timeout_clocks == u64::MAX {
        Duration::from_secs(31536000)
    } else {
        let nanos = (timeout_clocks as u128 * 1_000_000_000) / frequency as u128;
        Duration::from_nanos(nanos as u64)
    };

    let start = Instant::now();
    let spin_duration = Duration::from_micros(200);

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

        let elapsed = start.elapsed();
        if elapsed > timeout_duration {
            return signals.len();
        }

        if wait_hint == HsaWaitState::Active || elapsed < spin_duration {
            std::hint::spin_loop();
            continue;
        }

        let wait_ms = timeout_duration
            .saturating_sub(elapsed)
            .as_millis()
            .min(u32::MAX as u128) as u32;

        let _ = event_manager.wait_on_multiple_events(device, &events_ref, false, wait_ms);
    }
}
