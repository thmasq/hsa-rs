use std::io;
// Assuming the path to your implemented module is 'thunk::context'
use hsa_rs::thunk::context;
use hsa_rs::thunk::topology::{
    HSA_HEAPTYPE_DEVICE_SVM, HSA_HEAPTYPE_FRAME_BUFFER_PRIVATE, HSA_HEAPTYPE_FRAME_BUFFER_PUBLIC,
    HSA_HEAPTYPE_GPU_GDS, HSA_HEAPTYPE_GPU_LDS, HSA_HEAPTYPE_GPU_SCRATCH, HSA_HEAPTYPE_MMIO_REMAP,
    HSA_HEAPTYPE_SYSTEM, HsaMemoryProperties,
};

// Helper function to convert the numeric heap type to a human-readable string
fn get_heap_name(heap_type: u32) -> &'static str {
    match heap_type {
        HSA_HEAPTYPE_SYSTEM => "SYSTEM",
        HSA_HEAPTYPE_FRAME_BUFFER_PUBLIC => "VRAM (Public)",
        HSA_HEAPTYPE_FRAME_BUFFER_PRIVATE => "VRAM (Private)",
        HSA_HEAPTYPE_GPU_GDS => "GPU GDS",
        HSA_HEAPTYPE_GPU_LDS => "GPU LDS",
        HSA_HEAPTYPE_GPU_SCRATCH => "GPU Scratch",
        HSA_HEAPTYPE_DEVICE_SVM => "Device SVM",
        HSA_HEAPTYPE_MMIO_REMAP => "MMIO Remap",
        _ => "Unknown",
    }
}

// Helper to display memory bank details
fn print_memory_properties(props: &[HsaMemoryProperties]) {
    if props.is_empty() {
        println!("  - No memory banks reported.");
        return;
    }

    for prop in props {
        let size_mb = prop.size_in_bytes / 1024 / 1024;
        let heap_name = get_heap_name(prop.heap_type);
        println!("  - Heap Type: {:<15} | Size: {} MB", heap_name, size_mb);
    }
}

fn main() -> io::Result<()> {
    println!("--- HSA Runtime Context Acquisition ---");

    // 1. Acquire the global runtime context
    let context = match context::acquire() {
        Ok(ctx) => ctx,
        Err(e) => {
            eprintln!(
                "Error acquiring HSA context (Is KFD installed and loaded?): {}",
                e
            );
            // Attempt to release any partial initialization on error
            context::release();
            return Err(e);
        }
    };

    println!("\n--- System Properties ---");
    println!("Number of Nodes: {}", context.system_properties.num_nodes);
    // Print other system-wide properties...

    // 2. Iterate over the discovered nodes
    println!("\n--- Discovered Nodes ---");
    for node in &context.nodes {
        println!("\n[Node ID: {}]", node.node_id);

        // Print properties from the base HsaNodeProperties
        println!("  GPU ID:            {}", node.properties.kfd_gpu_id);
        println!("  ISA Name:          {}", node.isa_name);
        println!("  CPU Cores:         {}", node.properties.cpu_cores_count);
        println!("  SIMD Count:        {}", node.properties.simd_count);
        println!("  Max CP Queues:     {}", node.properties.num_cp_queues);
        println!("  LDS Size:          {} KB", node.properties.lds_size_in_kb);

        // Print extracted memory properties
        println!("  Memory Properties ({})", node.mem_properties.len());
        print_memory_properties(&node.mem_properties);

        // Optionally print other properties:
        // println!("  Cache Properties ({})", node.cache_properties.len());
        // ...
        // println!("  IO Link Properties ({})", node.io_link_properties.len());
        // ...
    }

    // 3. Release the context once finished (optional, but good practice for clean shutdown)
    context::release();
    println!("\n--- Context Released ---");

    Ok(())
}
