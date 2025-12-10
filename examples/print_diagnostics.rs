use hsa_rs::kfd::device::KfdDevice;
use hsa_rs::kfd::ioctl::{GetProcessAperturesNewArgs, ProcessDeviceApertures};
use hsa_rs::kfd::topology::{
    HSA_IOLINKTYPE_NUMA, HSA_IOLINKTYPE_PCIEXPRESS, HSA_IOLINKTYPE_XGMI, Topology,
};
use std::fs::{self};
use std::io;
use std::os::unix::io::AsRawFd;

fn main() -> io::Result<()> {
    println!("============================================================");
    println!("             Rust KFD Driver Port - Diagnostics             ");
    println!("============================================================");

    // 1. Open the Driver
    println!("[+] Opening /dev/kfd...");
    let kfd = match KfdDevice::open() {
        Ok(dev) => dev,
        Err(e) => {
            eprintln!("[-] Failed to open /dev/kfd: {}", e);
            eprintln!("    (Ensure the 'amdgpu' kernel module is loaded and you have permissions)");
            return Err(e);
        }
    };

    // 2. Check Version
    let version = kfd.get_version()?;
    println!(
        "[+] KFD Interface Version: {}.{}",
        version.major_version, version.minor_version
    );

    // 3. Topology Snapshot
    println!("\n[+] Scanning System Topology...");
    let topology = Topology::get_snapshot()?;
    println!("    Generation ID: {}", Topology::get_generation_id()?);
    println!("    System Nodes:  {}", topology.system_props.num_nodes);
    println!("    Platform ID:   {}", topology.system_props.platform_id);

    let mut gpu_nodes = Vec::new();

    for node in &topology.nodes {
        println!("\n------------------------------------------------------------");
        println!(
            " Node {} ({})",
            node.properties.node_id, node.properties.marketing_name
        );
        println!("------------------------------------------------------------");

        // Basic Properties
        if node.properties.simd_count > 0 {
            println!("    Type:          GPU");
            println!(
                "    Engine ID:     {}.{}.{}",
                node.properties.engine_id.major,
                node.properties.engine_id.minor,
                node.properties.engine_id.stepping
            );
            println!("    SIMDs:         {}", node.properties.simd_count);
            println!("    Waves/SIMD:    {}", node.properties.max_waves_per_simd);
            println!(
                "    VRAM Size:     {} MB",
                node.properties.local_mem_size / 1024 / 1024
            );
            println!("    KFD GPU ID:    {}", node.properties.kfd_gpu_id);
            println!("    Location ID:   0x{:x}", node.properties.location_id);

            // Store for VM test later
            gpu_nodes.push(node);
        } else {
            println!("    Type:          CPU");
            println!("    Cores:         {}", node.properties.cpu_cores_count);
        }

        // Memory Banks
        if !node.mem_banks.is_empty() {
            println!("\n    Memory Banks:");
            for (i, mem) in node.mem_banks.iter().enumerate() {
                let type_str = match mem.heap_type {
                    0 => "System",
                    1 => "FrameBuffer (VRAM)",
                    _ => "Unknown",
                };
                println!(
                    "      [{}] {:<20} Size: {} MB",
                    i,
                    type_str,
                    mem.size_in_bytes / 1024 / 1024
                );
            }
        }

        // Caches
        if !node.caches.is_empty() {
            println!("\n    Caches:");
            for cache in &node.caches {
                println!(
                    "      L{} Size: {} KB, Assoc: {}",
                    cache.cache_level,
                    cache.cache_size / 1024,
                    cache.cache_associativity
                );
            }
        }

        // IO Links
        if !node.io_links.is_empty() {
            println!("\n    IO Links:");
            for link in &node.io_links {
                let type_str = match link.type_ {
                    HSA_IOLINKTYPE_PCIEXPRESS => "PCIe",
                    HSA_IOLINKTYPE_XGMI => "XGMI",
                    HSA_IOLINKTYPE_NUMA => "NUMA",
                    _ => "Indirect/Other",
                };
                println!(
                    "      -> Node {:<2} | Type: {:<8} | Weight: {:<3} | Min/Max Latency: {}/{}",
                    link.node_to, type_str, link.weight, link.min_latency, link.max_latency
                );
            }
        }
    }

    // 4. Test Driver Interaction (Process Apertures)
    println!("\n[+] Testing Process Apertures...");

    // We allocate space for the aperture info in Rust
    // The kernel will fill this struct.
    let mut apertures: Vec<ProcessDeviceApertures> = Vec::with_capacity(topology.nodes.len());
    // We need to initialize the vector with default values safely to pass the pointer
    for _ in 0..topology.nodes.len() {
        apertures.push(ProcessDeviceApertures::default());
    }

    let mut args = GetProcessAperturesNewArgs {
        kfd_process_device_apertures_ptr: apertures.as_mut_ptr() as u64,
        num_of_nodes: topology.nodes.len() as u32,
        pad: 0,
    };

    match kfd.get_process_apertures_new(&mut args) {
        Ok(_) => {
            println!(
                "    Success. Retrieved apertures for {} nodes.",
                args.num_of_nodes
            );
            for (i, ap) in apertures
                .iter()
                .take(args.num_of_nodes as usize)
                .enumerate()
            {
                if ap.gpu_id != 0 {
                    println!(
                        "    Node {} (GPU ID {}): LDS [0x{:x} - 0x{:x}] Scratch [0x{:x} - 0x{:x}]",
                        i, ap.gpu_id, ap.lds_base, ap.lds_limit, ap.scratch_base, ap.scratch_limit
                    );
                }
            }
        }
        Err(e) => println!(
            "    [-] Failed to get apertures: {} (This is expected if not running inside a ROCm-setup process)",
            e
        ),
    }

    // 5. Test VM Acquisition (Requires DRM)
    if !gpu_nodes.is_empty() {
        println!("\n[+] Testing VM Acquisition for first GPU...");
        let gpu = &gpu_nodes[0];

        // Construct the DRM path.
        // Note: In a real runtime, we map PCI bus ID/Location ID to the correct renderDxx file.
        // Here we just guess /dev/dri/renderD128 for the first GPU to demonstrate the ioctl.
        let drm_path = "/dev/dri/renderD128";

        println!("    Attempting to open {}", drm_path);
        match fs::OpenOptions::new().read(true).write(true).open(drm_path) {
            Ok(drm_file) => {
                println!(
                    "    DRM Device opened (fd: {}). calling AMDKFD_IOC_ACQUIRE_VM...",
                    drm_file.as_raw_fd()
                );

                match kfd.acquire_vm(gpu.properties.kfd_gpu_id, drm_file.as_raw_fd() as u32) {
                    Ok(_) => println!(
                        "    [SUCCESS] VM Acquired! This process is now bound to GPU {}.",
                        gpu.properties.kfd_gpu_id
                    ),
                    Err(e) => println!(
                        "    [FAILURE] Acquire VM failed: {}. (Is the GPU in use or permissions incorrect?)",
                        e
                    ),
                }
            }
            Err(e) => println!(
                "    [-] Could not open {}: {}. Skipping VM test.",
                drm_path, e
            ),
        }
    } else {
        println!("\n[-] No GPU nodes found to test VM acquisition.");
    }

    Ok(())
}
