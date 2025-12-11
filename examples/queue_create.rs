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
    // This reserves the Virtual Address apertures (SVM, etc.)
    println!("[+] Initializing Memory Manager (FMM)...");
    let mut mem_mgr = MemoryManager::new(&device, &node_props)
        .map_err(|e| format!("Failed to initialize MemoryManager (Err: {})", e))?;

    // 4. Select a GPU Node
    // We search for the first node that has SIMD cores.
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
    // The Thunk QueueBuilder expects us to provide the memory for the ring itself.
    // We allocate 64KB in GTT (System Memory), which is accessible by both Host and Device.
    let ring_size = 64 * 1024;
    println!("[+] Allocating {} KB Ring Buffer...", ring_size / 1024);

    let ring_mem = mem_mgr
        .allocate_gtt(&device, ring_size, gpu_idx as u32, drm_file.as_raw_fd())
        .map_err(|e| format!("Ring buffer allocation failed (Err: {})", e))?;

    println!("    GPU VA:  0x{:012x}", ring_mem.gpu_va);
    println!("    CPU Ptr: {:?}", ring_mem.ptr);

    // 6. Build the Queue
    // This triggers the heavy lifting: Allocating CWSR/EOP buffers and mapping Doorbells.
    println!("[+] creating Compute Queue...");
    let builder = QueueBuilder::new(
        &device,
        &mut mem_mgr,
        &gpu_node.properties,
        gpu_idx as u32,
        drm_file.as_raw_fd(),
        ring_mem.gpu_va,
        ring_size as u64,
    )
    .with_type(QueueType::Compute)
    .with_priority(QueuePriority::Normal);

    let queue = builder
        .create()
        .map_err(|e| format!("Queue creation failed (Err: {})", e))?;

    // 7. Verify Success
    println!("============================================================");
    println!(" [SUCCESS] Queue Created!");
    println!("============================================================");
    println!("    Queue ID:        {}", queue.queue_id);
    println!("    Doorbell VA:     0x{:012x}", queue.queue_doorbell);
    println!("    Read Ptr VA:     0x{:012x}", queue.queue_read_ptr);
    println!("    Write Ptr VA:    0x{:012x}", queue.queue_write_ptr);

    // Note: To actually execute work, you would now:
    // 1. Write PM4 packets into `ring_mem.ptr`
    // 2. Update the write pointer at `queue.queue_write_ptr`
    // 3. Write to the doorbell at `queue.queue_doorbell`

    // 8. Cleanup
    println!("\n[+] cleaning up resources...");

    // The HsaQueue struct now implements Drop.
    // When `queue` goes out of scope here (or is dropped explicitly), it will:
    // 1. Destroy the KFD Queue
    // 2. Free the internal EOP and CWSR buffers
    drop(queue);
    println!("    Queue destroyed and internal resources freed");

    // Free the Ring Buffer
    // This was allocated manually by us via mem_mgr, so we must free it manually.
    mem_mgr.free_memory(&device, ring_mem.handle);
    println!("    Ring buffer freed");

    Ok(())
}
