use crate::kfd::device::KfdDevice;
use crate::kfd::sysfs::EngineId;
use crate::thunk::topology::{
    HsaCacheProperties, HsaIoLinkProperties, HsaMemoryProperties, HsaNodeProperties,
    HsaSystemProperties, acquire_system_properties, get_node_cache_properties,
    get_node_io_link_properties, get_node_memory_properties, get_node_properties,
    release_system_properties,
};
use std::io;
use std::sync::{Arc, Mutex};

// ===============================================================================================
// Context Structures
// ===============================================================================================

/// A complete, processed snapshot of a single HSA node's properties, ready for runtime use.
#[derive(Debug, Clone)]
pub struct Node {
    /// The global node ID (corresponds to the index in the Context node list).
    pub node_id: u32,
    /// Kernel-reported immutable node properties.
    pub properties: HsaNodeProperties,
    /// The calculated ISA name string (e.g., "gfx900", "gfx1030").
    pub isa_name: String,
    /// List of memory regions (heaps) available to this node.
    pub mem_properties: Vec<HsaMemoryProperties>,
    /// List of cache attributes.
    pub cache_properties: Vec<HsaCacheProperties>,
    /// List of inter-node links (e.g., `PCIe`, xGMI).
    pub io_link_properties: Vec<HsaIoLinkProperties>,
}

/// The global runtime context, encapsulating the KFD device handle and the system topology.
///
/// This acts as the singleton for the entire thunk layer, initialized on the first call to `acquire`.
#[derive(Debug)]
pub struct Context {
    /// The open file descriptor to `/dev/kfd`.
    pub device: Arc<KfdDevice>,
    /// System-wide properties.
    pub system_properties: HsaSystemProperties,
    /// A consolidated list of all initialized nodes.
    pub nodes: Vec<Node>,
}

// ===============================================================================================
// Global Singleton Management
// ===============================================================================================

static GLOBAL_CONTEXT: Mutex<Option<Arc<Context>>> = Mutex::new(None);

/// Acquires and initializes the global HSA runtime context if it has not already been created.
///
/// This is the primary entry point to initialize the KFD connection and scan the system topology.
///
/// # Errors
/// Returns an `io::Error` if the KFD device cannot be opened or if the topology scan fails.
///
/// # Panics
/// Panics if the internal mutex is poisoned.
pub fn acquire() -> io::Result<Arc<Context>> {
    let mut guard = GLOBAL_CONTEXT.lock().unwrap();

    if let Some(ctx) = guard.as_ref() {
        return Ok(ctx.clone());
    }

    // 1. Open the KFD device
    let kfd_device = KfdDevice::open()?;

    // 2. Acquire system properties (this internally drives the initial topology scan)
    let system_props = acquire_system_properties()?;

    // 3. Build the full Node list by querying all properties for each node
    let mut nodes = Vec::new();
    let num_nodes = system_props.num_nodes;

    // Iterate through node IDs 0 to num_nodes - 1 (Fix for Error 1: no 'nodes' field on HsaSystemProperties)
    for node_id in 0..num_nodes {
        // Fetch the low-level properties for this node ID
        let node_props = get_node_properties(node_id)?;

        // Query extended properties
        let num_mem_banks = node_props.mem_banks_count;
        let num_caches = node_props.cpu_cores_count; // Fix for Error 2: use cpu_cores_count
        let num_links = node_props.io_links_count; // Fix for Error 3: use io_links_count

        let mem_properties = get_node_memory_properties(node_id, num_mem_banks)?;
        let cache_properties = get_node_cache_properties(node_id, 0, num_caches)?;
        let io_link_properties = get_node_io_link_properties(node_id, num_links)?;
        let isa_name = get_isa_name(&node_props.engine_id);

        nodes.push(Node {
            node_id,
            properties: node_props,
            isa_name,
            mem_properties,
            cache_properties,
            io_link_properties,
        });
    }

    // 4. Cache and return the new context
    let context = Arc::new(Context {
        device: Arc::new(kfd_device),
        system_properties: system_props,
        nodes,
    });

    *guard = Some(context.clone());
    drop(guard);

    Ok(context)
}

/// Releases the global HSA runtime context.
///
/// This should typically be called on shutdown. It closes the KFD file descriptor.
///
/// # Panics
/// Panics if the internal mutex is poisoned.
pub fn release() {
    // Drop the content, including the Arc<KfdDevice>, which closes the file descriptor.
    GLOBAL_CONTEXT.lock().unwrap().take();
    // Also clear the cached topology properties as they are tied to the context state
    release_system_properties();
}

// ===============================================================================================
// Helper Functions
// ===============================================================================================

/// Generates a GFX ISA version string from the KFD `EngineId`.
fn get_isa_name(engine_id: &EngineId) -> String {
    // This calculation is commonly used in ROCm runtimes to get the GFX version number.
    let major = engine_id.major;
    let minor = engine_id.minor;
    let stepping = engine_id.stepping;

    match major {
        // Assume non-GPU agents (CPUs) don't have an ISA name in this context.
        0 => "cpu".to_string(),
        // Simple case: gfx900, gfx1010, gfx1100 format
        _ => format!("gfx{}{}{}", major, minor, stepping),
    }
}
