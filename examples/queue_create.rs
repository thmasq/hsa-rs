use hsa_rs::kfd::device::KfdDevice;
use hsa_rs::kfd::sysfs::Topology;
use hsa_rs::thunk::memory::MemoryManager;
use hsa_rs::thunk::queues::builder::{QueueBuilder, QueuePriority, QueueType};
use std::fs::OpenOptions;
use std::os::unix::io::AsRawFd;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("============================================================");
    println!("             HSA Rust Thunk - Queue Creation Test           ");
    println!("============================================================");

    // 1. Open KFD Device
    println!("[+] Opening /dev/kfd...");
    let device = KfdDevice::open()?;
    let version = device.get_version()?;
    println!(
        "    KFD Interface Version: {}.{}",
        version.major_version, version.minor_version
    );

    // 2. Discover Topology
    println!("[+] Scanning System Topology...");
    let topology = Topology::get_snapshot()?;

    // We need to extract the properties to initialize the Memory Manager
    let node_props: Vec<_> = topology
        .nodes
        .iter()
        .map(|n| n.properties.clone())
        .collect();

    // 3. Initialize Memory Manager (FMM)
    // Returns Arc<Mutex<MemoryManager>>
    println!("[+] Initializing Memory Manager (FMM)...");
    let mem_mgr_arc = MemoryManager::new(&device, &node_props)
        .map_err(|e| format!("Failed to initialize MemoryManager (Err: {})", e))?;

    // 4. Select a GPU Node
    let gpu_idx = topology
        .nodes
        .iter()
        .position(|n| n.properties.simd_count > 0)
        .ok_or("No GPU nodes found in topology")?;

    let gpu_node = &topology.nodes[gpu_idx];
    let gpu_id = gpu_node.properties.kfd_gpu_id;

    println!("[+] Selected Node {} (GPU ID: {})", gpu_idx, gpu_id);
    println!("    Name: {}", gpu_node.properties.marketing_name);

    let drm_minor = gpu_node.properties.drm_render_minor;
    if drm_minor < 0 {
        return Err("Invalid DRM render minor number".into());
    }

    let drm_path = format!("/dev/dri/renderD{}", drm_minor);
    println!("[+] Opening DRM Device: {}", drm_path);

    let drm_file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&drm_path)
        .map_err(|e| format!("Failed to open {}: {}", drm_path, e))?;

    println!("[+] Acquiring VM...");
    device.acquire_vm(gpu_id, drm_file.as_raw_fd() as u32)?;

    // 5. Allocate Ring Buffer
    // We allocate 64KB in GTT (System Memory).
    let ring_size = 64 * 1024;
    println!("[+] Allocating {} KB Ring Buffer...", ring_size / 1024);

    let ring_mem = {
        let mut guard = mem_mgr_arc.lock().unwrap();
        guard
            .allocate_gtt(&device, ring_size, gpu_idx as u32, drm_file.as_raw_fd())
            .map_err(|e| format!("Ring buffer allocation failed (Err: {})", e))?
    };

    println!("    GPU VA:  0x{:012x}", ring_mem.gpu_va);
    println!("    CPU Ptr: {:?}", ring_mem.ptr);

    // 6. Build the Queue
    println!("[+] creating Compute Queue...");

    let queue = {
        let mut guard = mem_mgr_arc.lock().unwrap();

        QueueBuilder::new(
            &device,
            &mut *guard, // Pass mutable reference to the manager
            &gpu_node.properties,
            gpu_idx as u32,
            drm_file.as_raw_fd(),
            ring_mem.gpu_va,
            ring_size as u64,
        )
        .with_type(QueueType::Compute)
        .with_priority(QueuePriority::Normal)
        .create()
        .map_err(|e| format!("Queue creation failed (Err: {})", e))?
    };

    // 7. Verify Success
    println!("============================================================");
    println!(" [SUCCESS] Queue Created!");
    println!("============================================================");
    println!("    Queue ID:        {}", queue.queue_id);
    println!("    Doorbell VA:     0x{:012x}", queue.queue_doorbell);
    println!("    Read Ptr VA:     0x{:012x}", queue.queue_read_ptr);
    println!("    Write Ptr VA:    0x{:012x}", queue.queue_write_ptr);

    // 8. Cleanup
    println!("\n[+] cleaning up resources...");

    // The HsaQueue struct implements Drop.
    // Because we scoped the locks above, the Mutex is FREE.
    // When `queue` drops, it triggers Allocation::drop -> locks MemoryManager -> Success.
    drop(queue);
    println!("    Queue destroyed and internal resources freed");

    // Free the Ring Buffer
    // Same here: Allocation::drop locks MemoryManager -> Success.
    drop(ring_mem);
    println!("    Ring buffer freed (RAII)");

    Ok(())
}
