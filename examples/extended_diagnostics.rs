use hsa_rs::kfd::sysfs::{HSA_IOLINKTYPE_NUMA, HSA_IOLINKTYPE_PCIEXPRESS, HSA_IOLINKTYPE_XGMI};
use hsa_rs::thunk::topology::{
    self, HSA_HEAPTYPE_DEVICE_SVM, HSA_HEAPTYPE_FRAME_BUFFER_PRIVATE,
    HSA_HEAPTYPE_FRAME_BUFFER_PUBLIC, HSA_HEAPTYPE_GPU_GDS, HSA_HEAPTYPE_GPU_LDS,
    HSA_HEAPTYPE_GPU_SCRATCH, HSA_HEAPTYPE_MMIO_REMAP, HSA_HEAPTYPE_SYSTEM,
};
use std::io;

fn main() -> io::Result<()> {
    println!("============================================================");
    println!("        Rust HSA Thunk Topology - Extended Diagnostics      ");
    println!("============================================================");

    // 1. Acquire System Properties
    // This triggers the Thunk's topology snapshot logic:
    // - Retries on generation_id mismatch
    // - Parses Sysfs
    // - Enriches CPU info from /proc/cpuinfo
    // - Calculates indirect IO links
    // - Queries KFD IOCTLs for process apertures (LDS/Scratch limits)
    println!("[+] Acquiring System Properties (Thunk API)...");
    let sys_props = topology::acquire_system_properties()?;

    println!("    System Nodes: {}", sys_props.num_nodes);
    println!("    Platform ID:  {}", sys_props.platform_id);
    println!("    Platform Oem: {}", sys_props.platform_oem);
    println!("    Platform Rev: {}", sys_props.platform_rev);
    println!("    Timestamp Freq:: {}", sys_props.timestamp_frequency);

    // 2. Iterate Nodes using Thunk Getters
    for i in 0..sys_props.num_nodes {
        // Get Node Properties
        // This returns the enriched HsaNodeProperties (with correct mem_banks_count)
        let node_props = topology::get_node_properties(i)?;

        println!("\n------------------------------------------------------------");
        println!(
            " Node {} ({})",
            node_props.node_id, node_props.marketing_name
        );
        println!("------------------------------------------------------------");

        // Basic Properties
        if node_props.simd_count > 0 {
            println!("    Type:          GPU");
            println!("    ASIC Name:     {}", node_props.amd_name);
            println!(
                "    Engine ID:     {}.{}.{}",
                node_props.engine_id.major,
                node_props.engine_id.minor,
                node_props.engine_id.stepping
            );
            println!("    SIMDs:         {}", node_props.simd_count);
            println!("    Shader Banks:  {}", node_props.num_shader_banks);
            println!("    Waves/SIMD:    {}", node_props.max_waves_per_simd);
            println!("    KFD GPU ID:    {}", node_props.kfd_gpu_id);
        } else {
            println!("    Type:          CPU");
            println!("    Cores:         {}", node_props.cpu_cores_count);
        }

        // Memory Properties
        // Note: We use the count from the properties to request the list.
        // This list will contain dynamic apertures (LDS, Scratch) not found in Sysfs.
        let num_banks = node_props.mem_banks_count;
        if num_banks > 0 {
            println!("\n    Memory Banks ({}):", num_banks);
            let mem_props = topology::get_node_memory_properties(i, num_banks)?;

            for (idx, mem) in mem_props.iter().enumerate() {
                let (type_str, is_virtual) = match mem.heap_type {
                    HSA_HEAPTYPE_SYSTEM => ("System RAM", false),
                    HSA_HEAPTYPE_FRAME_BUFFER_PUBLIC => ("VRAM (Public)", false),
                    HSA_HEAPTYPE_FRAME_BUFFER_PRIVATE => ("VRAM (Private)", false),
                    HSA_HEAPTYPE_GPU_GDS => ("GDS", false),
                    HSA_HEAPTYPE_GPU_LDS => ("LDS", true),
                    HSA_HEAPTYPE_GPU_SCRATCH => ("Scratch", true),
                    HSA_HEAPTYPE_DEVICE_SVM => ("SVM", true),
                    HSA_HEAPTYPE_MMIO_REMAP => ("MMIO Remap", true),
                    _ => ("Unknown", false),
                };

                let note = if is_virtual { "(Aperture)" } else { "" };

                println!(
                    "      [{}] {:<15} {:<10} Size: {:<10}",
                    idx,
                    type_str,
                    note,
                    format_size(mem.size_in_bytes),
                );
            }
        }

        // Cache Properties
        let num_caches = node_props.caches_count;
        if num_caches > 0 {
            println!("\n    Caches (Top {}):", num_caches);
            let caches = topology::get_node_cache_properties(i, 0, num_caches)?;
            for cache in caches {
                println!(
                    "      L{} Size: {:<8} Assoc: {}",
                    cache.cache_level,
                    format_size(cache.cache_size as u64),
                    cache.cache_associativity
                );
            }
        }

        // IO Links
        // Includes indirect links calculated by the Thunk
        let num_links = node_props.io_links_count;
        if num_links > 0 {
            println!("\n    IO Links (Total {}):", num_links);
            let links = topology::get_node_io_link_properties(i, num_links)?;
            for link in links {
                let type_str = match link.type_ {
                    HSA_IOLINKTYPE_PCIEXPRESS => "PCIe",
                    HSA_IOLINKTYPE_XGMI => "XGMI",
                    HSA_IOLINKTYPE_NUMA => "NUMA",
                    _ => "Other",
                };
                println!(
                    "      -> Node {:<2} | {:<5} | Weight: {:<3} | Bandwidth: {} - {}",
                    link.node_to, type_str, link.weight, link.min_bandwidth, link.max_bandwidth
                );
            }
        }
    }

    // 3. Release Snapshot
    // In Rust this happens automatically if we dropped the struct, but we expose
    // the C-style API explicitly.
    topology::release_system_properties();
    println!("\n[+] System Properties Released.");

    Ok(())
}

fn format_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.2} GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1024 * 1024 {
        format!("{:.2} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.2} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}
