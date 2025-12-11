use crate::kfd::device::KfdDevice;
use crate::kfd::ioctl::{
    GetProcessAperturesArgs, GetProcessAperturesNewArgs, NUM_OF_SUPPORTED_GPUS,
    ProcessDeviceApertures,
};
use crate::kfd::sysfs::{self, Topology as SysfsTopology};
pub use crate::kfd::sysfs::{
    HsaCacheProperties, HsaIoLinkProperties, HsaMemoryProperties, HsaNodeProperties,
    HsaSystemProperties,
};
use std::collections::HashMap;
use std::io;
use std::sync::{Arc, Mutex};

// ===============================================================================================
// Constants (Thunk Specific)
// ===============================================================================================

pub const HSA_HEAPTYPE_SYSTEM: u32 = 0;
pub const HSA_HEAPTYPE_FRAME_BUFFER_PUBLIC: u32 = 1;
pub const HSA_HEAPTYPE_FRAME_BUFFER_PRIVATE: u32 = 2;
pub const HSA_HEAPTYPE_GPU_GDS: u32 = 3;
pub const HSA_HEAPTYPE_GPU_LDS: u32 = 4;
pub const HSA_HEAPTYPE_GPU_SCRATCH: u32 = 5;
pub const HSA_HEAPTYPE_DEVICE_SVM: u32 = 6;
pub const HSA_HEAPTYPE_MMIO_REMAP: u32 = 7;

const GFX_VERSION_VEGA10: u32 = 90000;
const GFX_VERSION_KAVERI: u32 = 70000;

// ===============================================================================================
// Extended Topology Data
// ===============================================================================================

/// Stores dynamic aperture limits queried from KFD IOCTLs.
/// These are NOT in sysfs and must be queried per process.
#[derive(Debug, Clone, Default)]
struct NodeApertures {
    lds_base: u64,
    lds_limit: u64,
    scratch_base: u64,
    scratch_limit: u64,
    gpuvm_base: u64,
    gpuvm_limit: u64,
}

/// The runtime topology snapshot.
/// Wraps the static Sysfs topology and adds dynamic runtime info.
#[derive(Debug, Clone)]
pub struct Topology {
    inner: SysfsTopology,
    apertures: HashMap<u32, NodeApertures>,
    is_dgpu: bool,
}

// Global singleton to match libhsakmt's g_system / g_props
static GLOBAL_TOPOLOGY: Mutex<Option<Arc<Topology>>> = Mutex::new(None);

// ===============================================================================================
// Implementation
// ===============================================================================================

impl Topology {
    /// Captures the system topology.
    /// 1. Reads Sysfs (reusing kfd::sysfs logic).
    /// 2. Checks generation_id for consistency.
    /// 3. Queries KFD for process apertures.
    fn new() -> io::Result<Self> {
        let mut retries = 0;
        loop {
            // Sysfs topology generation check loop
            let gen_start = SysfsTopology::get_generation_id().unwrap_or(0);
            let sys_topo = SysfsTopology::get_snapshot()?;
            let gen_end = SysfsTopology::get_generation_id().unwrap_or(0);

            if gen_start == gen_end || retries > 5 {
                // Determine dGPU status (Any node with SIMDs but no CPU cores)
                let is_dgpu = sys_topo
                    .nodes
                    .iter()
                    .any(|n| n.properties.simd_count > 0 && n.properties.cpu_cores_count == 0);

                // Fetch aperture limits via IOCTL
                let apertures = Self::fetch_apertures(&sys_topo.nodes)?;

                return Ok(Self {
                    inner: sys_topo,
                    apertures,
                    is_dgpu,
                });
            }
            retries += 1;
        }
    }

    /// Queries KFD IOCTLs to get the virtual address ranges for LDS, Scratch, etc.
    fn fetch_apertures(nodes: &[sysfs::Node]) -> io::Result<HashMap<u32, NodeApertures>> {
        let kfd = KfdDevice::open()?;
        let mut map = HashMap::new();

        let gpu_nodes: Vec<u32> = nodes
            .iter()
            .filter(|n| n.properties.simd_count > 0 && n.properties.kfd_gpu_id > 0)
            .map(|n| n.properties.kfd_gpu_id)
            .collect();

        if gpu_nodes.is_empty() {
            return Ok(map);
        }

        let num_nodes = gpu_nodes.len() as u32;
        let mut aps_vec = vec![ProcessDeviceApertures::default(); num_nodes as usize];

        // Attempt New API
        let mut args_new = GetProcessAperturesNewArgs {
            kfd_process_device_apertures_ptr: aps_vec.as_mut_ptr() as u64,
            num_of_nodes: num_nodes,
            pad: 0,
        };

        if kfd.get_process_apertures_new(&mut args_new).is_ok() {
            for ap in aps_vec {
                if ap.gpu_id != 0 {
                    map.insert(ap.gpu_id, Self::convert_aperture(&ap));
                }
            }
        } else {
            // Fallback to Old API (Limit 7 GPUs)
            let mut args_old = GetProcessAperturesArgs {
                process_apertures: [ProcessDeviceApertures::default(); NUM_OF_SUPPORTED_GPUS],
                num_of_nodes: NUM_OF_SUPPORTED_GPUS as u32,
                pad: 0,
            };
            if kfd.get_process_apertures(&mut args_old).is_ok() {
                for i in 0..NUM_OF_SUPPORTED_GPUS {
                    let ap = &args_old.process_apertures[i];
                    if ap.gpu_id != 0 {
                        map.insert(ap.gpu_id, Self::convert_aperture(ap));
                    }
                }
            }
        }

        Ok(map)
    }

    fn convert_aperture(src: &ProcessDeviceApertures) -> NodeApertures {
        NodeApertures {
            lds_base: src.lds_base,
            lds_limit: src.lds_limit,
            scratch_base: src.scratch_base,
            scratch_limit: src.scratch_limit,
            gpuvm_base: src.gpuvm_base,
            gpuvm_limit: src.gpuvm_limit,
        }
    }

    /// Helper to decide if SVM memory bank should be synthesized
    fn is_svm_needed(&self, props: &HsaNodeProperties) -> bool {
        if self.is_dgpu {
            return true;
        }
        let ver =
            props.engine_id.major * 10000 + props.engine_id.minor * 100 + props.engine_id.stepping;
        ver >= GFX_VERSION_VEGA10
    }
}

// ===============================================================================================
// Public API Functions
// ===============================================================================================

pub fn acquire_system_properties() -> io::Result<HsaSystemProperties> {
    let mut guard = GLOBAL_TOPOLOGY.lock().unwrap();
    if guard.is_none() {
        let topo = Topology::new()?;
        *guard = Some(Arc::new(topo));
    }
    Ok(guard.as_ref().unwrap().inner.system_props.clone())
}

pub fn release_system_properties() {
    let mut guard = GLOBAL_TOPOLOGY.lock().unwrap();
    *guard = None;
}

pub fn get_node_properties(node_id: u32) -> io::Result<HsaNodeProperties> {
    let guard = GLOBAL_TOPOLOGY.lock().unwrap();
    let topo = guard
        .as_ref()
        .ok_or_else(|| io::Error::from(io::ErrorKind::NotConnected))?;

    let node = topo
        .inner
        .nodes
        .get(node_id as usize)
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;

    let mut props = node.properties.clone();

    // Adjust memory bank count to include virtual heaps (LDS, Scratch, SVM)
    // Matches topology.c: hsaKmtGetNodePropertiesCtx logic
    if props.kfd_gpu_id != 0 {
        if topo.is_dgpu {
            props.mem_banks_count += 3;
        } else {
            props.mem_banks_count += 3;
        }
        // MMIO check usually adds 1
        props.mem_banks_count += 1;
    }

    Ok(props)
}

pub fn get_node_memory_properties(
    node_id: u32,
    num_banks: u32,
) -> io::Result<Vec<HsaMemoryProperties>> {
    let guard = GLOBAL_TOPOLOGY.lock().unwrap();
    let topo = guard
        .as_ref()
        .ok_or_else(|| io::Error::from(io::ErrorKind::NotConnected))?;

    let node = topo
        .inner
        .nodes
        .get(node_id as usize)
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;

    let mut props = Vec::new();

    // 1. Add Static Sysfs Banks
    for bank in &node.mem_banks {
        if props.len() >= num_banks as usize {
            break;
        }
        props.push(bank.clone());
    }

    if node.properties.kfd_gpu_id == 0 {
        return Ok(props);
    }

    // 2. Add Dynamic Apertures
    if let Some(ap) = topo.apertures.get(&node.properties.kfd_gpu_id) {
        // LDS
        if props.len() < num_banks as usize && ap.lds_limit > ap.lds_base {
            props.push(HsaMemoryProperties {
                heap_type: HSA_HEAPTYPE_GPU_LDS,
                size_in_bytes: (node.properties.lds_size_in_kb as u64) * 1024,
                flags: 0,
                width: 0,
                mem_clk_max: 0,
            });
        }

        // Local Memory (Private) for Kaveri Legacy
        let ver = node.properties.engine_id.major * 10000
            + node.properties.engine_id.minor * 100
            + node.properties.engine_id.stepping;

        if ver == GFX_VERSION_KAVERI
            && props.len() < num_banks as usize
            && node.properties.local_mem_size > 0
        {
            props.push(HsaMemoryProperties {
                heap_type: HSA_HEAPTYPE_FRAME_BUFFER_PRIVATE,
                size_in_bytes: node.properties.local_mem_size,
                flags: 0,
                width: 0,
                mem_clk_max: 0,
            });
        }

        // Scratch
        if props.len() < num_banks as usize && ap.scratch_limit > ap.scratch_base {
            props.push(HsaMemoryProperties {
                heap_type: HSA_HEAPTYPE_GPU_SCRATCH,
                size_in_bytes: (ap.scratch_limit - ap.scratch_base) + 1,
                flags: 0,
                width: 0,
                mem_clk_max: 0,
            });
        }

        // SVM (Shared Virtual Memory)
        if topo.is_svm_needed(&node.properties) && props.len() < num_banks as usize {
            let size = if ap.gpuvm_limit > ap.gpuvm_base {
                (ap.gpuvm_limit - ap.gpuvm_base) + 1
            } else {
                0
            };
            if size > 0 {
                props.push(HsaMemoryProperties {
                    heap_type: HSA_HEAPTYPE_DEVICE_SVM,
                    size_in_bytes: size,
                    flags: 0,
                    width: 0,
                    mem_clk_max: 0,
                });
            }
        }

        // MMIO Remap (Placeholder)
        if props.len() < num_banks as usize {
            props.push(HsaMemoryProperties {
                heap_type: HSA_HEAPTYPE_MMIO_REMAP,
                size_in_bytes: 4096, // Dummy size or fetch real aperture if available
                flags: 0,
                width: 0,
                mem_clk_max: 0,
            });
        }
    }

    Ok(props)
}

pub fn get_node_cache_properties(
    node_id: u32,
    _proc_id: u32,
    num_caches: u32,
) -> io::Result<Vec<HsaCacheProperties>> {
    let guard = GLOBAL_TOPOLOGY.lock().unwrap();
    let topo = guard
        .as_ref()
        .ok_or_else(|| io::Error::from(io::ErrorKind::NotConnected))?;

    let node = topo
        .inner
        .nodes
        .get(node_id as usize)
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;

    let count = std::cmp::min(node.caches.len(), num_caches as usize);
    Ok(node.caches[..count].to_vec())
}

pub fn get_node_io_link_properties(
    node_id: u32,
    num_links: u32,
) -> io::Result<Vec<HsaIoLinkProperties>> {
    let guard = GLOBAL_TOPOLOGY.lock().unwrap();
    let topo = guard
        .as_ref()
        .ok_or_else(|| io::Error::from(io::ErrorKind::NotConnected))?;

    let node = topo
        .inner
        .nodes
        .get(node_id as usize)
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;

    let count = std::cmp::min(node.io_links.len(), num_links as usize);
    Ok(node.io_links[..count].to_vec())
}
