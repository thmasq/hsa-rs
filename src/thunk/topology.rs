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
#[derive(Debug, Clone)]
pub struct Topology {
    inner: SysfsTopology,
    apertures: HashMap<u32, NodeApertures>,
    is_dgpu: bool,
}

static GLOBAL_TOPOLOGY: Mutex<Option<Arc<Topology>>> = Mutex::new(None);

// ===============================================================================================
// Implementation
// ===============================================================================================

impl Topology {
    /// Captures the system topology.
    ///
    /// # Panics
    /// Panics if reading the sysfs topology locking logic internally panics.
    fn new() -> io::Result<Self> {
        let mut retries = 0;
        loop {
            let gen_start = SysfsTopology::get_generation_id().unwrap_or(0);
            let sys_topo = SysfsTopology::get_snapshot()?;
            let gen_end = SysfsTopology::get_generation_id().unwrap_or(0);

            if gen_start == gen_end || retries > 5 {
                let is_dgpu = sys_topo
                    .nodes
                    .iter()
                    .any(|n| n.properties.simd_count > 0 && n.properties.cpu_cores_count == 0);

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

        #[allow(clippy::cast_possible_truncation)]
        let num_nodes = gpu_nodes.len() as u32;
        let mut aps_vec = vec![ProcessDeviceApertures::default(); num_nodes as usize];

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
            let mut args_old = GetProcessAperturesArgs {
                process_apertures: [ProcessDeviceApertures::default(); NUM_OF_SUPPORTED_GPUS],
                #[allow(clippy::cast_possible_truncation)]
                num_of_nodes: NUM_OF_SUPPORTED_GPUS as u32,
                pad: 0,
            };
            if kfd.get_process_apertures(&mut args_old).is_ok() {
                for ap in &args_old.process_apertures {
                    if ap.gpu_id != 0 {
                        map.insert(ap.gpu_id, Self::convert_aperture(ap));
                    }
                }
            }
        }

        Ok(map)
    }

    const fn convert_aperture(src: &ProcessDeviceApertures) -> NodeApertures {
        NodeApertures {
            lds_base: src.lds_base,
            lds_limit: src.lds_limit,
            scratch_base: src.scratch_base,
            scratch_limit: src.scratch_limit,
            gpuvm_base: src.gpuvm_base,
            gpuvm_limit: src.gpuvm_limit,
        }
    }

    const fn is_svm_needed(&self, props: &HsaNodeProperties) -> bool {
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

/// Acquires and initializes the global system properties.
///
/// # Panics
/// Panics if the global topology mutex is poisoned.
pub fn acquire_system_properties() -> io::Result<HsaSystemProperties> {
    let mut guard = GLOBAL_TOPOLOGY.lock().unwrap();
    if guard.is_none() {
        let topo = Topology::new()?;
        *guard = Some(Arc::new(topo));
    }
    Ok(guard.as_ref().unwrap().inner.system_props.clone())
}

/// Releases the global topology.
///
/// # Panics
/// Panics if the global topology mutex is poisoned.
pub fn release_system_properties() {
    GLOBAL_TOPOLOGY.lock().unwrap().take();
}

/// Returns properties for a node.
///
/// # Panics
/// Panics if the global topology mutex is poisoned.
pub fn get_node_properties(node_id: u32) -> io::Result<HsaNodeProperties> {
    let topo = GLOBAL_TOPOLOGY
        .lock()
        .unwrap()
        .as_ref()
        .ok_or_else(|| io::Error::from(io::ErrorKind::NotConnected))?
        .clone();

    let node = topo
        .inner
        .nodes
        .get(node_id as usize)
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;

    let mut props = node.properties.clone();

    if props.kfd_gpu_id != 0 {
        // Unified branch — was identical on both sides
        props.mem_banks_count += 3; // LDS + Scratch + SVM
        props.mem_banks_count += 1; // MMIO
    }

    Ok(props)
}

/// Returns memory banks for a node.
///
/// # Panics
/// Panics if the global topology mutex is poisoned.
pub fn get_node_memory_properties(
    node_id: u32,
    num_banks: u32,
) -> io::Result<Vec<HsaMemoryProperties>> {
    let topo = GLOBAL_TOPOLOGY
        .lock()
        .unwrap()
        .as_ref()
        .ok_or_else(|| io::Error::from(io::ErrorKind::NotConnected))?
        .clone();

    let node = topo
        .inner
        .nodes
        .get(node_id as usize)
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;

    let mut props = Vec::new();

    for bank in &node.mem_banks {
        if props.len() >= num_banks as usize {
            break;
        }
        props.push(bank.clone());
    }

    if node.properties.kfd_gpu_id == 0 {
        return Ok(props);
    }

    if let Some(ap) = topo.apertures.get(&node.properties.kfd_gpu_id) {
        if props.len() < num_banks as usize && ap.lds_limit > ap.lds_base {
            props.push(HsaMemoryProperties {
                heap_type: HSA_HEAPTYPE_GPU_LDS,
                size_in_bytes: u64::from(node.properties.lds_size_in_kb) * 1024,
                flags: 0,
                width: 0,
                mem_clk_max: 0,
            });
        }

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

        if props.len() < num_banks as usize && ap.scratch_limit > ap.scratch_base {
            props.push(HsaMemoryProperties {
                heap_type: HSA_HEAPTYPE_GPU_SCRATCH,
                size_in_bytes: (ap.scratch_limit - ap.scratch_base) + 1,
                flags: 0,
                width: 0,
                mem_clk_max: 0,
            });
        }

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

        if props.len() < num_banks as usize {
            props.push(HsaMemoryProperties {
                heap_type: HSA_HEAPTYPE_MMIO_REMAP,
                size_in_bytes: 4096,
                flags: 0,
                width: 0,
                mem_clk_max: 0,
            });
        }
    }

    Ok(props)
}

/// Returns cache properties.
///
/// # Panics
/// Panics if the global topology mutex is poisoned.
pub fn get_node_cache_properties(
    node_id: u32,
    _proc_id: u32,
    num_caches: u32,
) -> io::Result<Vec<HsaCacheProperties>> {
    let topo = GLOBAL_TOPOLOGY
        .lock()
        .unwrap()
        .as_ref()
        .ok_or_else(|| io::Error::from(io::ErrorKind::NotConnected))?
        .clone();

    let node = topo
        .inner
        .nodes
        .get(node_id as usize)
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;

    let count = std::cmp::min(node.caches.len(), num_caches as usize);
    Ok(node.caches[..count].to_vec())
}

/// Returns IO link properties.
///
/// # Panics
/// Panics if the global topology mutex is poisoned.
pub fn get_node_io_link_properties(
    node_id: u32,
    num_links: u32,
) -> io::Result<Vec<HsaIoLinkProperties>> {
    let topo = GLOBAL_TOPOLOGY
        .lock()
        .unwrap()
        .as_ref()
        .ok_or_else(|| io::Error::from(io::ErrorKind::NotConnected))?
        .clone();

    let node = topo
        .inner
        .nodes
        .get(node_id as usize)
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;

    let count = std::cmp::min(node.io_links.len(), num_links as usize);
    Ok(node.io_links[..count].to_vec())
}
