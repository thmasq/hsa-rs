use hsa_rs::kfd::device::KfdDevice;
use hsa_rs::kfd::sysfs::HsaNodeProperties;
use hsa_rs::thunk::events::{EventManager, HsaEventDescriptor, HsaEventType, HsaSyncVar};
use hsa_rs::thunk::memory::MemoryManager;
use std::fs::OpenOptions;
use std::os::fd::AsRawFd;
use std::ptr;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Starting Event Infrastructure Test ===");

    // 1. Initialize KFD Device
    let device = KfdDevice::open()?;
    println!("[+] Opened KFD device");

    // 2. Open DRM Render Node
    let drm_file = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/dri/renderD128")
        .expect("Failed to open /dev/dri/renderD128. Ensure your user is in the 'render' or 'video' group.");
    let drm_fd = drm_file.as_raw_fd();
    println!("[+] Opened DRM render node");

    // 3. Load Topology (Simulated)
    let mut mock_node = HsaNodeProperties::default();
    mock_node.kfd_gpu_id = 0x1234; // Arbitrary non-zero ID
    let nodes = vec![HsaNodeProperties::default(), mock_node];

    // 4. Initialize Managers
    // map_err is used here to convert the i32 error code into a generic Box<dyn Error>
    let mut memory_manager = MemoryManager::new(&device, &nodes)
        .map_err(|e| format!("Failed to init Memory Manager: {}", e))?;
    let mut event_manager = EventManager::new(&nodes);
    println!("[+] Managers initialized");

    // 5. Create a Signal Event
    let event_desc = HsaEventDescriptor {
        event_type: HsaEventType::Signal,
        node_id: 1, // Use the GPU node
        sync_var: HsaSyncVar {
            user_data: ptr::null_mut(),
            sync_var_size: 0,
        },
    };

    let mut event = event_manager
        .create_event(
            &device,
            &mut memory_manager,
            drm_fd,
            &event_desc,
            true,  // Manual Reset
            false, // Is Signaled
        )
        .map_err(|e| format!("Failed to create event: {}", e))?;

    println!("[+] Created Event ID: {}", event.event_id);
    println!("    HW Event Page Slot: {}", event.hw_data2);

    // 6. TEST 1: Wait Timeout
    println!("\n[TEST 1] Waiting on unsignaled event (Expect Timeout)...");
    let mut events_to_wait = vec![&mut event];
    let result = event_manager.wait_on_multiple_events(&device, &mut events_to_wait, false, 500);

    match result {
        Err(-31) => println!("    SUCCESS: Timed out as expected."),
        Ok(_) => panic!("    FAILURE: Event signaled unexpectedly!"),
        Err(e) => panic!("    FAILURE: Unexpected error code: {}", e),
    }

    // 7. TEST 2: Signal and Wait
    println!("\n[TEST 2] Signaling event from CPU...");
    event_manager
        .set_event(&device, &event)
        .map_err(|e| format!("Failed to signal event: {}", e))?;

    println!("    Waiting for signal...");
    let mut events_to_wait = vec![&mut event];
    let result = event_manager
        .wait_on_multiple_events(&device, &mut events_to_wait, false, 1000)
        .map_err(|e| format!("Failed to wait on event: {}", e))?;

    if result.contains(&0) {
        println!("    SUCCESS: Event 0 signaled and woke up thread.");
    } else {
        panic!(
            "    FAILURE: Wait returned success but index list is empty/wrong: {:?}",
            result
        );
    }

    // 8. TEST 3: Event Age / State persistence
    println!("\n[TEST 3] Checking Manual Reset persistence...");
    let mut events_to_wait = vec![&mut event];
    let start = std::time::Instant::now();
    let result = event_manager
        .wait_on_multiple_events(&device, &mut events_to_wait, false, 1000)
        .map_err(|e| format!("Failed second wait: {}", e))?;

    if result.contains(&0) && start.elapsed().as_millis() < 100 {
        println!("    SUCCESS: Event remained signaled and returned immediately.");
    } else {
        panic!("    FAILURE: Event did not persist or timed out.");
    }

    // 9. Reset and Clean up
    println!("\n[Cleanup] Resetting and Destroying event...");
    event_manager
        .reset_event(&device, &event)
        .map_err(|e| format!("Failed to reset: {}", e))?;
    event_manager
        .destroy_event(&device, &event)
        .map_err(|e| format!("Failed to destroy: {}", e))?;

    println!("\n=== Test Complete: ALL SYSTEMS GO ===");
    Ok(())
}
