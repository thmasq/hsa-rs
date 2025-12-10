use std::collections::HashMap;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::str::FromStr;

const KFD_SYSFS_PATH: &str = "/sys/devices/virtual/kfd/kfd/topology";

// ===============================================================================================
// Constants & Lookups
// ===============================================================================================

pub const HSA_IOLINKTYPE_UNDEFINED: u32 = 0;
pub const HSA_IOLINKTYPE_PCIEXPRESS: u32 = 2;
pub const HSA_IOLINKTYPE_XGMI: u32 = 3;
pub const HSA_IOLINKTYPE_NUMA: u32 = 4;
pub const HSA_IOLINKTYPE_QPI_1_1: u32 = 5;

const SGPR_SIZE_PER_CU: u32 = 32 * 1024; // 32KB

struct GfxIpLookup {
    device_id: u16,
    major: u8,
    minor: u8,
    stepping: u8,
    name: &'static str,
}

const GFXIP_LOOKUP_TABLE: &[GfxIpLookup] = &[
    /* Kaveri Family */
    GfxIpLookup {
        device_id: 0x1304,
        major: 7,
        minor: 0,
        stepping: 0,
        name: "Spectre",
    },
    GfxIpLookup {
        device_id: 0x1305,
        major: 7,
        minor: 0,
        stepping: 0,
        name: "Spectre",
    },
    GfxIpLookup {
        device_id: 0x1306,
        major: 7,
        minor: 0,
        stepping: 0,
        name: "Spectre",
    },
    GfxIpLookup {
        device_id: 0x1307,
        major: 7,
        minor: 0,
        stepping: 0,
        name: "Spectre",
    },
    GfxIpLookup {
        device_id: 0x1309,
        major: 7,
        minor: 0,
        stepping: 0,
        name: "Spectre",
    },
    GfxIpLookup {
        device_id: 0x130A,
        major: 7,
        minor: 0,
        stepping: 0,
        name: "Spectre",
    },
    GfxIpLookup {
        device_id: 0x130B,
        major: 7,
        minor: 0,
        stepping: 0,
        name: "Spectre",
    },
    GfxIpLookup {
        device_id: 0x130C,
        major: 7,
        minor: 0,
        stepping: 0,
        name: "Spectre",
    },
    GfxIpLookup {
        device_id: 0x130D,
        major: 7,
        minor: 0,
        stepping: 0,
        name: "Spectre",
    },
    GfxIpLookup {
        device_id: 0x130E,
        major: 7,
        minor: 0,
        stepping: 0,
        name: "Spectre",
    },
    GfxIpLookup {
        device_id: 0x130F,
        major: 7,
        minor: 0,
        stepping: 0,
        name: "Spectre",
    },
    GfxIpLookup {
        device_id: 0x1310,
        major: 7,
        minor: 0,
        stepping: 0,
        name: "Spectre",
    },
    GfxIpLookup {
        device_id: 0x1311,
        major: 7,
        minor: 0,
        stepping: 0,
        name: "Spectre",
    },
    GfxIpLookup {
        device_id: 0x1312,
        major: 7,
        minor: 0,
        stepping: 0,
        name: "Spooky",
    },
    GfxIpLookup {
        device_id: 0x1313,
        major: 7,
        minor: 0,
        stepping: 0,
        name: "Spectre",
    },
    GfxIpLookup {
        device_id: 0x1315,
        major: 7,
        minor: 0,
        stepping: 0,
        name: "Spectre",
    },
    GfxIpLookup {
        device_id: 0x1316,
        major: 7,
        minor: 0,
        stepping: 0,
        name: "Spooky",
    },
    GfxIpLookup {
        device_id: 0x1317,
        major: 7,
        minor: 0,
        stepping: 0,
        name: "Spooky",
    },
    GfxIpLookup {
        device_id: 0x1318,
        major: 7,
        minor: 0,
        stepping: 0,
        name: "Spectre",
    },
    GfxIpLookup {
        device_id: 0x131B,
        major: 7,
        minor: 0,
        stepping: 0,
        name: "Spectre",
    },
    GfxIpLookup {
        device_id: 0x131C,
        major: 7,
        minor: 0,
        stepping: 0,
        name: "Spectre",
    },
    GfxIpLookup {
        device_id: 0x131D,
        major: 7,
        minor: 0,
        stepping: 0,
        name: "Spectre",
    },
    /* Hawaii Family */
    GfxIpLookup {
        device_id: 0x67A0,
        major: 7,
        minor: 0,
        stepping: 1,
        name: "Hawaii",
    },
    GfxIpLookup {
        device_id: 0x67A1,
        major: 7,
        minor: 0,
        stepping: 1,
        name: "Hawaii",
    },
    GfxIpLookup {
        device_id: 0x67A2,
        major: 7,
        minor: 0,
        stepping: 1,
        name: "Hawaii",
    },
    GfxIpLookup {
        device_id: 0x67A8,
        major: 7,
        minor: 0,
        stepping: 1,
        name: "Hawaii",
    },
    GfxIpLookup {
        device_id: 0x67A9,
        major: 7,
        minor: 0,
        stepping: 1,
        name: "Hawaii",
    },
    GfxIpLookup {
        device_id: 0x67AA,
        major: 7,
        minor: 0,
        stepping: 1,
        name: "Hawaii",
    },
    GfxIpLookup {
        device_id: 0x67B0,
        major: 7,
        minor: 0,
        stepping: 1,
        name: "Hawaii",
    },
    GfxIpLookup {
        device_id: 0x67B1,
        major: 7,
        minor: 0,
        stepping: 1,
        name: "Hawaii",
    },
    GfxIpLookup {
        device_id: 0x67B8,
        major: 7,
        minor: 0,
        stepping: 1,
        name: "Hawaii",
    },
    GfxIpLookup {
        device_id: 0x67B9,
        major: 7,
        minor: 0,
        stepping: 1,
        name: "Hawaii",
    },
    GfxIpLookup {
        device_id: 0x67BA,
        major: 7,
        minor: 0,
        stepping: 1,
        name: "Hawaii",
    },
    GfxIpLookup {
        device_id: 0x67BE,
        major: 7,
        minor: 0,
        stepping: 1,
        name: "Hawaii",
    },
    /* Carrizo Family */
    GfxIpLookup {
        device_id: 0x9870,
        major: 8,
        minor: 0,
        stepping: 1,
        name: "Carrizo",
    },
    GfxIpLookup {
        device_id: 0x9874,
        major: 8,
        minor: 0,
        stepping: 1,
        name: "Carrizo",
    },
    GfxIpLookup {
        device_id: 0x9875,
        major: 8,
        minor: 0,
        stepping: 1,
        name: "Carrizo",
    },
    GfxIpLookup {
        device_id: 0x9876,
        major: 8,
        minor: 0,
        stepping: 1,
        name: "Carrizo",
    },
    GfxIpLookup {
        device_id: 0x9877,
        major: 8,
        minor: 0,
        stepping: 1,
        name: "Carrizo",
    },
    /* Tonga Family */
    GfxIpLookup {
        device_id: 0x6920,
        major: 8,
        minor: 0,
        stepping: 2,
        name: "Tonga",
    },
    GfxIpLookup {
        device_id: 0x6921,
        major: 8,
        minor: 0,
        stepping: 2,
        name: "Tonga",
    },
    GfxIpLookup {
        device_id: 0x6928,
        major: 8,
        minor: 0,
        stepping: 2,
        name: "Tonga",
    },
    GfxIpLookup {
        device_id: 0x6929,
        major: 8,
        minor: 0,
        stepping: 2,
        name: "Tonga",
    },
    GfxIpLookup {
        device_id: 0x692B,
        major: 8,
        minor: 0,
        stepping: 2,
        name: "Tonga",
    },
    GfxIpLookup {
        device_id: 0x692F,
        major: 8,
        minor: 0,
        stepping: 2,
        name: "Tonga",
    },
    GfxIpLookup {
        device_id: 0x6930,
        major: 8,
        minor: 0,
        stepping: 2,
        name: "Tonga",
    },
    GfxIpLookup {
        device_id: 0x6938,
        major: 8,
        minor: 0,
        stepping: 2,
        name: "Tonga",
    },
    GfxIpLookup {
        device_id: 0x6939,
        major: 8,
        minor: 0,
        stepping: 2,
        name: "Tonga",
    },
    /* Fiji */
    GfxIpLookup {
        device_id: 0x7300,
        major: 8,
        minor: 0,
        stepping: 3,
        name: "Fiji",
    },
    GfxIpLookup {
        device_id: 0x730F,
        major: 8,
        minor: 0,
        stepping: 3,
        name: "Fiji",
    },
    /* Polaris10 */
    GfxIpLookup {
        device_id: 0x67C0,
        major: 8,
        minor: 0,
        stepping: 3,
        name: "Polaris10",
    },
    GfxIpLookup {
        device_id: 0x67C1,
        major: 8,
        minor: 0,
        stepping: 3,
        name: "Polaris10",
    },
    GfxIpLookup {
        device_id: 0x67C2,
        major: 8,
        minor: 0,
        stepping: 3,
        name: "Polaris10",
    },
    GfxIpLookup {
        device_id: 0x67C4,
        major: 8,
        minor: 0,
        stepping: 3,
        name: "Polaris10",
    },
    GfxIpLookup {
        device_id: 0x67C7,
        major: 8,
        minor: 0,
        stepping: 3,
        name: "Polaris10",
    },
    GfxIpLookup {
        device_id: 0x67C8,
        major: 8,
        minor: 0,
        stepping: 3,
        name: "Polaris10",
    },
    GfxIpLookup {
        device_id: 0x67C9,
        major: 8,
        minor: 0,
        stepping: 3,
        name: "Polaris10",
    },
    GfxIpLookup {
        device_id: 0x67CA,
        major: 8,
        minor: 0,
        stepping: 3,
        name: "Polaris10",
    },
    GfxIpLookup {
        device_id: 0x67CC,
        major: 8,
        minor: 0,
        stepping: 3,
        name: "Polaris10",
    },
    GfxIpLookup {
        device_id: 0x67CF,
        major: 8,
        minor: 0,
        stepping: 3,
        name: "Polaris10",
    },
    GfxIpLookup {
        device_id: 0x67D0,
        major: 8,
        minor: 0,
        stepping: 3,
        name: "Polaris10",
    },
    GfxIpLookup {
        device_id: 0x67DF,
        major: 8,
        minor: 0,
        stepping: 3,
        name: "Polaris10",
    },
    GfxIpLookup {
        device_id: 0x6FDF,
        major: 8,
        minor: 0,
        stepping: 3,
        name: "Polaris10",
    },
    /* Polaris11 */
    GfxIpLookup {
        device_id: 0x67E0,
        major: 8,
        minor: 0,
        stepping: 3,
        name: "Polaris11",
    },
    GfxIpLookup {
        device_id: 0x67E1,
        major: 8,
        minor: 0,
        stepping: 3,
        name: "Polaris11",
    },
    GfxIpLookup {
        device_id: 0x67E3,
        major: 8,
        minor: 0,
        stepping: 3,
        name: "Polaris11",
    },
    GfxIpLookup {
        device_id: 0x67E7,
        major: 8,
        minor: 0,
        stepping: 3,
        name: "Polaris11",
    },
    GfxIpLookup {
        device_id: 0x67E8,
        major: 8,
        minor: 0,
        stepping: 3,
        name: "Polaris11",
    },
    GfxIpLookup {
        device_id: 0x67E9,
        major: 8,
        minor: 0,
        stepping: 3,
        name: "Polaris11",
    },
    GfxIpLookup {
        device_id: 0x67EB,
        major: 8,
        minor: 0,
        stepping: 3,
        name: "Polaris11",
    },
    GfxIpLookup {
        device_id: 0x67EF,
        major: 8,
        minor: 0,
        stepping: 3,
        name: "Polaris11",
    },
    GfxIpLookup {
        device_id: 0x67FF,
        major: 8,
        minor: 0,
        stepping: 3,
        name: "Polaris11",
    },
    /* Polaris12 */
    GfxIpLookup {
        device_id: 0x6980,
        major: 8,
        minor: 0,
        stepping: 3,
        name: "Polaris12",
    },
    GfxIpLookup {
        device_id: 0x6981,
        major: 8,
        minor: 0,
        stepping: 3,
        name: "Polaris12",
    },
    GfxIpLookup {
        device_id: 0x6985,
        major: 8,
        minor: 0,
        stepping: 3,
        name: "Polaris12",
    },
    GfxIpLookup {
        device_id: 0x6986,
        major: 8,
        minor: 0,
        stepping: 3,
        name: "Polaris12",
    },
    GfxIpLookup {
        device_id: 0x6987,
        major: 8,
        minor: 0,
        stepping: 3,
        name: "Polaris12",
    },
    GfxIpLookup {
        device_id: 0x6995,
        major: 8,
        minor: 0,
        stepping: 3,
        name: "Polaris12",
    },
    GfxIpLookup {
        device_id: 0x6997,
        major: 8,
        minor: 0,
        stepping: 3,
        name: "Polaris12",
    },
    GfxIpLookup {
        device_id: 0x699F,
        major: 8,
        minor: 0,
        stepping: 3,
        name: "Polaris12",
    },
    /* VegaM */
    GfxIpLookup {
        device_id: 0x694C,
        major: 8,
        minor: 0,
        stepping: 3,
        name: "VegaM",
    },
    GfxIpLookup {
        device_id: 0x694E,
        major: 8,
        minor: 0,
        stepping: 3,
        name: "VegaM",
    },
    GfxIpLookup {
        device_id: 0x694F,
        major: 8,
        minor: 0,
        stepping: 3,
        name: "VegaM",
    },
    /* Vega10 */
    GfxIpLookup {
        device_id: 0x6860,
        major: 9,
        minor: 0,
        stepping: 0,
        name: "Vega10",
    },
    GfxIpLookup {
        device_id: 0x6861,
        major: 9,
        minor: 0,
        stepping: 0,
        name: "Vega10",
    },
    GfxIpLookup {
        device_id: 0x6862,
        major: 9,
        minor: 0,
        stepping: 0,
        name: "Vega10",
    },
    GfxIpLookup {
        device_id: 0x6863,
        major: 9,
        minor: 0,
        stepping: 0,
        name: "Vega10",
    },
    GfxIpLookup {
        device_id: 0x6864,
        major: 9,
        minor: 0,
        stepping: 0,
        name: "Vega10",
    },
    GfxIpLookup {
        device_id: 0x6867,
        major: 9,
        minor: 0,
        stepping: 0,
        name: "Vega10",
    },
    GfxIpLookup {
        device_id: 0x6868,
        major: 9,
        minor: 0,
        stepping: 0,
        name: "Vega10",
    },
    GfxIpLookup {
        device_id: 0x6869,
        major: 9,
        minor: 0,
        stepping: 0,
        name: "Vega10",
    },
    GfxIpLookup {
        device_id: 0x686A,
        major: 9,
        minor: 0,
        stepping: 0,
        name: "Vega10",
    },
    GfxIpLookup {
        device_id: 0x686B,
        major: 9,
        minor: 0,
        stepping: 0,
        name: "Vega10",
    },
    GfxIpLookup {
        device_id: 0x686C,
        major: 9,
        minor: 0,
        stepping: 0,
        name: "Vega10",
    },
    GfxIpLookup {
        device_id: 0x686D,
        major: 9,
        minor: 0,
        stepping: 0,
        name: "Vega10",
    },
    GfxIpLookup {
        device_id: 0x686E,
        major: 9,
        minor: 0,
        stepping: 0,
        name: "Vega10",
    },
    GfxIpLookup {
        device_id: 0x687F,
        major: 9,
        minor: 0,
        stepping: 0,
        name: "Vega10",
    },
    /* Vega12 */
    GfxIpLookup {
        device_id: 0x69A0,
        major: 9,
        minor: 0,
        stepping: 4,
        name: "Vega12",
    },
    GfxIpLookup {
        device_id: 0x69A1,
        major: 9,
        minor: 0,
        stepping: 4,
        name: "Vega12",
    },
    GfxIpLookup {
        device_id: 0x69A2,
        major: 9,
        minor: 0,
        stepping: 4,
        name: "Vega12",
    },
    GfxIpLookup {
        device_id: 0x69A3,
        major: 9,
        minor: 0,
        stepping: 4,
        name: "Vega12",
    },
    GfxIpLookup {
        device_id: 0x69Af,
        major: 9,
        minor: 0,
        stepping: 4,
        name: "Vega12",
    },
    /* Raven */
    GfxIpLookup {
        device_id: 0x15DD,
        major: 9,
        minor: 0,
        stepping: 2,
        name: "Raven",
    },
    GfxIpLookup {
        device_id: 0x15D8,
        major: 9,
        minor: 0,
        stepping: 2,
        name: "Raven",
    },
    /* Vega20 */
    GfxIpLookup {
        device_id: 0x66A0,
        major: 9,
        minor: 0,
        stepping: 6,
        name: "Vega20",
    },
    GfxIpLookup {
        device_id: 0x66A1,
        major: 9,
        minor: 0,
        stepping: 6,
        name: "Vega20",
    },
    GfxIpLookup {
        device_id: 0x66A2,
        major: 9,
        minor: 0,
        stepping: 6,
        name: "Vega20",
    },
    GfxIpLookup {
        device_id: 0x66A3,
        major: 9,
        minor: 0,
        stepping: 6,
        name: "Vega20",
    },
    GfxIpLookup {
        device_id: 0x66A4,
        major: 9,
        minor: 0,
        stepping: 6,
        name: "Vega20",
    },
    GfxIpLookup {
        device_id: 0x66A7,
        major: 9,
        minor: 0,
        stepping: 6,
        name: "Vega20",
    },
    GfxIpLookup {
        device_id: 0x66AF,
        major: 9,
        minor: 0,
        stepping: 6,
        name: "Vega20",
    },
    /* Arcturus */
    GfxIpLookup {
        device_id: 0x7388,
        major: 9,
        minor: 0,
        stepping: 8,
        name: "Arcturus",
    },
    GfxIpLookup {
        device_id: 0x738C,
        major: 9,
        minor: 0,
        stepping: 8,
        name: "Arcturus",
    },
    GfxIpLookup {
        device_id: 0x738E,
        major: 9,
        minor: 0,
        stepping: 8,
        name: "Arcturus",
    },
    GfxIpLookup {
        device_id: 0x7390,
        major: 9,
        minor: 0,
        stepping: 8,
        name: "Arcturus",
    },
    /* Aldebaran */
    GfxIpLookup {
        device_id: 0x7408,
        major: 9,
        minor: 0,
        stepping: 10,
        name: "Aldebaran",
    },
    GfxIpLookup {
        device_id: 0x740C,
        major: 9,
        minor: 0,
        stepping: 10,
        name: "Aldebaran",
    },
    GfxIpLookup {
        device_id: 0x740F,
        major: 9,
        minor: 0,
        stepping: 10,
        name: "Aldebaran",
    },
    GfxIpLookup {
        device_id: 0x7410,
        major: 9,
        minor: 0,
        stepping: 10,
        name: "Aldebaran",
    },
    /* Renoir */
    GfxIpLookup {
        device_id: 0x15E7,
        major: 9,
        minor: 0,
        stepping: 12,
        name: "Renoir",
    },
    GfxIpLookup {
        device_id: 0x1636,
        major: 9,
        minor: 0,
        stepping: 12,
        name: "Renoir",
    },
    GfxIpLookup {
        device_id: 0x1638,
        major: 9,
        minor: 0,
        stepping: 12,
        name: "Renoir",
    },
    GfxIpLookup {
        device_id: 0x164C,
        major: 9,
        minor: 0,
        stepping: 12,
        name: "Renoir",
    },
    /* Navi10 */
    GfxIpLookup {
        device_id: 0x7310,
        major: 10,
        minor: 1,
        stepping: 0,
        name: "Navi10",
    },
    GfxIpLookup {
        device_id: 0x7312,
        major: 10,
        minor: 1,
        stepping: 0,
        name: "Navi10",
    },
    GfxIpLookup {
        device_id: 0x7318,
        major: 10,
        minor: 1,
        stepping: 0,
        name: "Navi10",
    },
    GfxIpLookup {
        device_id: 0x731A,
        major: 10,
        minor: 1,
        stepping: 0,
        name: "Navi10",
    },
    GfxIpLookup {
        device_id: 0x731E,
        major: 10,
        minor: 1,
        stepping: 0,
        name: "Navi10",
    },
    GfxIpLookup {
        device_id: 0x731F,
        major: 10,
        minor: 1,
        stepping: 0,
        name: "Navi10",
    },
    /* cyan_skillfish */
    GfxIpLookup {
        device_id: 0x13F9,
        major: 10,
        minor: 1,
        stepping: 3,
        name: "cyan_skillfish",
    },
    GfxIpLookup {
        device_id: 0x13FA,
        major: 10,
        minor: 1,
        stepping: 3,
        name: "cyan_skillfish",
    },
    GfxIpLookup {
        device_id: 0x13FB,
        major: 10,
        minor: 1,
        stepping: 3,
        name: "cyan_skillfish",
    },
    GfxIpLookup {
        device_id: 0x13FC,
        major: 10,
        minor: 1,
        stepping: 3,
        name: "cyan_skillfish",
    },
    GfxIpLookup {
        device_id: 0x13FE,
        major: 10,
        minor: 1,
        stepping: 3,
        name: "cyan_skillfish",
    },
    GfxIpLookup {
        device_id: 0x143F,
        major: 10,
        minor: 1,
        stepping: 3,
        name: "cyan_skillfish",
    },
    /* Navi14 */
    GfxIpLookup {
        device_id: 0x7340,
        major: 10,
        minor: 1,
        stepping: 2,
        name: "Navi14",
    },
    GfxIpLookup {
        device_id: 0x7341,
        major: 10,
        minor: 1,
        stepping: 2,
        name: "Navi14",
    },
    GfxIpLookup {
        device_id: 0x7347,
        major: 10,
        minor: 1,
        stepping: 2,
        name: "Navi14",
    },
    /* Navi12 */
    GfxIpLookup {
        device_id: 0x7360,
        major: 10,
        minor: 1,
        stepping: 1,
        name: "Navi12",
    },
    GfxIpLookup {
        device_id: 0x7362,
        major: 10,
        minor: 1,
        stepping: 1,
        name: "Navi12",
    },
    /* SIENNA_CICHLID */
    GfxIpLookup {
        device_id: 0x73A0,
        major: 10,
        minor: 3,
        stepping: 0,
        name: "SIENNA_CICHLID",
    },
    GfxIpLookup {
        device_id: 0x73A1,
        major: 10,
        minor: 3,
        stepping: 0,
        name: "SIENNA_CICHLID",
    },
    GfxIpLookup {
        device_id: 0x73A2,
        major: 10,
        minor: 3,
        stepping: 0,
        name: "SIENNA_CICHLID",
    },
    GfxIpLookup {
        device_id: 0x73A3,
        major: 10,
        minor: 3,
        stepping: 0,
        name: "SIENNA_CICHLID",
    },
    GfxIpLookup {
        device_id: 0x73A5,
        major: 10,
        minor: 3,
        stepping: 0,
        name: "SIENNA_CICHLID",
    },
    GfxIpLookup {
        device_id: 0x73A8,
        major: 10,
        minor: 3,
        stepping: 0,
        name: "SIENNA_CICHLID",
    },
    GfxIpLookup {
        device_id: 0x73A9,
        major: 10,
        minor: 3,
        stepping: 0,
        name: "SIENNA_CICHLID",
    },
    GfxIpLookup {
        device_id: 0x73AC,
        major: 10,
        minor: 3,
        stepping: 0,
        name: "SIENNA_CICHLID",
    },
    GfxIpLookup {
        device_id: 0x73AD,
        major: 10,
        minor: 3,
        stepping: 0,
        name: "SIENNA_CICHLID",
    },
    GfxIpLookup {
        device_id: 0x73AB,
        major: 10,
        minor: 3,
        stepping: 0,
        name: "SIENNA_CICHLID",
    },
    GfxIpLookup {
        device_id: 0x73AE,
        major: 10,
        minor: 3,
        stepping: 0,
        name: "SIENNA_CICHLID",
    },
    GfxIpLookup {
        device_id: 0x73BF,
        major: 10,
        minor: 3,
        stepping: 0,
        name: "SIENNA_CICHLID",
    },
    /* NAVY_FLOUNDER */
    GfxIpLookup {
        device_id: 0x73C0,
        major: 10,
        minor: 3,
        stepping: 1,
        name: "NAVY_FLOUNDER",
    },
    GfxIpLookup {
        device_id: 0x73C1,
        major: 10,
        minor: 3,
        stepping: 1,
        name: "NAVY_FLOUNDER",
    },
    GfxIpLookup {
        device_id: 0x73C3,
        major: 10,
        minor: 3,
        stepping: 1,
        name: "NAVY_FLOUNDER",
    },
    GfxIpLookup {
        device_id: 0x73DA,
        major: 10,
        minor: 3,
        stepping: 1,
        name: "NAVY_FLOUNDER",
    },
    GfxIpLookup {
        device_id: 0x73DB,
        major: 10,
        minor: 3,
        stepping: 1,
        name: "NAVY_FLOUNDER",
    },
    GfxIpLookup {
        device_id: 0x73DC,
        major: 10,
        minor: 3,
        stepping: 1,
        name: "NAVY_FLOUNDER",
    },
    GfxIpLookup {
        device_id: 0x73DD,
        major: 10,
        minor: 3,
        stepping: 1,
        name: "NAVY_FLOUNDER",
    },
    GfxIpLookup {
        device_id: 0x73DE,
        major: 10,
        minor: 3,
        stepping: 1,
        name: "NAVY_FLOUNDER",
    },
    GfxIpLookup {
        device_id: 0x73DF,
        major: 10,
        minor: 3,
        stepping: 1,
        name: "NAVY_FLOUNDER",
    },
    /* DIMGREY_CAVEFISH */
    GfxIpLookup {
        device_id: 0x73E0,
        major: 10,
        minor: 3,
        stepping: 2,
        name: "DIMGREY_CAVEFISH",
    },
    GfxIpLookup {
        device_id: 0x73E1,
        major: 10,
        minor: 3,
        stepping: 2,
        name: "DIMGREY_CAVEFISH",
    },
    GfxIpLookup {
        device_id: 0x73E2,
        major: 10,
        minor: 3,
        stepping: 2,
        name: "DIMGREY_CAVEFISH",
    },
    GfxIpLookup {
        device_id: 0x73E8,
        major: 10,
        minor: 3,
        stepping: 2,
        name: "DIMGREY_CAVEFISH",
    },
    GfxIpLookup {
        device_id: 0x73E9,
        major: 10,
        minor: 3,
        stepping: 2,
        name: "DIMGREY_CAVEFISH",
    },
    GfxIpLookup {
        device_id: 0x73EA,
        major: 10,
        minor: 3,
        stepping: 2,
        name: "DIMGREY_CAVEFISH",
    },
    GfxIpLookup {
        device_id: 0x73EB,
        major: 10,
        minor: 3,
        stepping: 2,
        name: "DIMGREY_CAVEFISH",
    },
    GfxIpLookup {
        device_id: 0x73EC,
        major: 10,
        minor: 3,
        stepping: 2,
        name: "DIMGREY_CAVEFISH",
    },
    GfxIpLookup {
        device_id: 0x73ED,
        major: 10,
        minor: 3,
        stepping: 2,
        name: "DIMGREY_CAVEFISH",
    },
    GfxIpLookup {
        device_id: 0x73EF,
        major: 10,
        minor: 3,
        stepping: 2,
        name: "DIMGREY_CAVEFISH",
    },
    GfxIpLookup {
        device_id: 0x73FF,
        major: 10,
        minor: 3,
        stepping: 2,
        name: "DIMGREY_CAVEFISH",
    },
    /* VanGogh */
    GfxIpLookup {
        device_id: 0x163F,
        major: 10,
        minor: 3,
        stepping: 3,
        name: "VanGogh",
    },
    /* BEIGE_GOBY */
    GfxIpLookup {
        device_id: 0x7420,
        major: 10,
        minor: 3,
        stepping: 4,
        name: "BEIGE_GOBY",
    },
    GfxIpLookup {
        device_id: 0x7421,
        major: 10,
        minor: 3,
        stepping: 4,
        name: "BEIGE_GOBY",
    },
    GfxIpLookup {
        device_id: 0x7422,
        major: 10,
        minor: 3,
        stepping: 4,
        name: "BEIGE_GOBY",
    },
    GfxIpLookup {
        device_id: 0x7423,
        major: 10,
        minor: 3,
        stepping: 4,
        name: "BEIGE_GOBY",
    },
    GfxIpLookup {
        device_id: 0x743F,
        major: 10,
        minor: 3,
        stepping: 4,
        name: "BEIGE_GOBY",
    },
    /* Yellow_Carp */
    GfxIpLookup {
        device_id: 0x164D,
        major: 10,
        minor: 3,
        stepping: 5,
        name: "YELLOW_CARP",
    },
    GfxIpLookup {
        device_id: 0x1681,
        major: 10,
        minor: 3,
        stepping: 5,
        name: "YELLOW_CARP",
    },
];

fn find_gfx_ip(device_id: u16, major_version: u8) -> Option<&'static GfxIpLookup> {
    // Sanity check matching C code logic
    if major_version > 12 {
        return None;
    }
    GFXIP_LOOKUP_TABLE
        .iter()
        .find(|entry| entry.device_id == device_id)
}

// Logic to emulate hsakmt_get_vgpr_size_per_cu based on GFX version
fn get_vgpr_size_per_cu(major: u32, minor: u32, stepping: u32) -> u32 {
    let full = (major << 16) | (minor << 8) | stepping;
    // Values derived from standard GCN/RDNA architectures
    if full >= 0x0A0000 {
        // GFX10+ (RDNA)
        // 32KB VGPRs per SIMD * 2 SIMDs per CU = 64KB?
        // Or 64KB physical file? ROCR runtime logic usually:
        262144 // 256KB Total Vector Register File per CU?
    } else {
        // GFX9 (Vega) and older: 64KB per SIMD * 4 SIMDs = 256KB
        262144
    }
}

// ===============================================================================================
// Data Structures
// ===============================================================================================

#[derive(Debug, Clone, Default)]
pub struct HsaSystemProperties {
    pub platform_oem: u32,
    pub platform_id: u32,
    pub platform_rev: u32,
    pub num_nodes: u32,
}

#[derive(Debug, Clone, Default)]
pub struct HsaNodeProperties {
    pub node_id: u32,

    // Core Counts
    pub cpu_cores_count: u32,
    pub simd_count: u32,
    pub mem_banks_count: u32,
    pub caches_count: u32,
    pub io_links_count: u32,
    pub p2p_links_count: u32,

    // Identifiers
    pub cpu_core_id_base: u32,
    pub simd_id_base: u32,
    pub vendor_id: u32,
    pub device_id: u32,
    pub location_id: u32,
    pub domain: u32,
    pub drm_render_minor: i32,
    pub hive_id: u64,
    pub unique_id: u64,
    pub kfd_gpu_id: u32,

    // Capabilities
    pub capability: u32,
    pub capability2: u32,
    pub debug_prop: u64,
    pub max_waves_per_simd: u32,
    pub lds_size_in_kb: u32,
    pub gds_size_in_kb: u32,
    pub wave_front_size: u32,

    // Memory
    pub local_mem_size: u64,

    // Architecture
    pub array_count: u32,
    pub simd_arrays_per_engine: u32,
    pub cu_per_simd_array: u32,
    pub simd_per_cu: u32,
    pub max_slots_scratch_cu: u32,
    pub fw_version: u32,
    pub gfx_target_version: u32,

    // Queues & Engines
    pub num_sdma_engines: u32,
    pub num_sdma_xgmi_engines: u32,
    pub num_gws: u32,
    pub num_sdma_queues_per_engine: u32,
    pub num_cp_queues: u32,
    pub num_xcc: u32,

    // Clocks
    pub max_engine_clk_fcompute: u32,
    pub max_engine_clk_ccompute: u32,

    // Enriched / Calculated Properties
    pub marketing_name: String,
    pub amd_name: String,
    pub engine_id: EngineId,
    pub num_shader_banks: u32,
    pub sgpr_size_per_cu: u32,
    pub vgpr_size_per_cu: u32,
}

#[derive(Debug, Clone, Default, Copy)]
pub struct EngineId {
    pub major: u32,
    pub minor: u32,
    pub stepping: u32,
}

#[derive(Debug, Clone, Default)]
pub struct HsaMemoryProperties {
    pub heap_type: u32,
    pub size_in_bytes: u64,
    pub flags: u32,
    pub width: u32,
    pub mem_clk_max: u32,
}

#[derive(Debug, Clone, Default)]
pub struct HsaCacheProperties {
    pub processor_id_low: u32,
    pub cache_level: u32,
    pub cache_size: u32,
    pub cache_line_size: u32,
    pub cache_lines_per_tag: u32,
    pub cache_associativity: u32,
    pub cache_latency: u32,
    pub cache_type: u32,
    pub sibling_map: Vec<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct HsaIoLinkProperties {
    pub type_: u32,
    pub version_major: u32,
    pub version_minor: u32,
    pub node_from: u32,
    pub node_to: u32,
    pub weight: u32,
    pub min_latency: u32,
    pub max_latency: u32,
    pub min_bandwidth: u32,
    pub max_bandwidth: u32,
    pub rec_transfer_size: u32,
    pub rec_sdma_eng_id_mask: u32,
    pub flags: u32,
}

#[derive(Debug, Clone)]
pub struct Topology {
    pub system_props: HsaSystemProperties,
    pub nodes: Vec<Node>,
}

#[derive(Debug, Clone)]
pub struct Node {
    pub properties: HsaNodeProperties,
    pub mem_banks: Vec<HsaMemoryProperties>,
    pub caches: Vec<HsaCacheProperties>,
    pub io_links: Vec<HsaIoLinkProperties>,
}

// ===============================================================================================
// Topology Implementation
// ===============================================================================================

impl Topology {
    pub fn get_snapshot() -> io::Result<Self> {
        let root = Path::new(KFD_SYSFS_PATH);
        if !root.exists() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "KFD topology not found",
            ));
        }

        let mut system_props = Self::parse_system_properties(&root.join("system_properties"))?;
        let cpu_info = Self::parse_cpu_info();

        let mut nodes = Vec::new();
        let nodes_dir = root.join("nodes");

        if let Ok(entries) = fs::read_dir(nodes_dir) {
            let mut paths: Vec<_> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
            // Sort by integer node ID to match sysfs order
            paths.sort_by_key(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .and_then(|s| s.parse::<u32>().ok())
                    .unwrap_or(u32::MAX)
            });

            for (idx, path) in paths.iter().enumerate() {
                if path.is_dir() {
                    if let Ok(mut node) = Node::from_sysfs(path) {
                        node.properties.node_id = idx as u32;

                        // Enrich CPU nodes with model name
                        if node.properties.cpu_cores_count > 0 {
                            if let Some(info) = cpu_info.get(&node.properties.cpu_core_id_base) {
                                node.properties.marketing_name = info.clone();
                                node.properties.amd_name = info.clone();
                            } else {
                                node.properties.marketing_name = "AMD CPU".to_string();
                            }
                        }

                        // Enrich GPU nodes (Overrides, Lookups, Derived Props)
                        Self::enrich_gpu_properties(&mut node.properties);

                        nodes.push(node);
                    }
                }
            }
        }

        // Calculate Indirect IO Links (GPU <-> CPU <-> GPU)
        // This requires visibility of all nodes, so must be done after parsing.
        let mut new_links = Vec::new();
        for i in 0..nodes.len() {
            for j in (i + 1)..nodes.len() {
                // Check i -> j
                if let Some(link) = Self::calculate_indirect_link(&nodes, i, j) {
                    new_links.push((i, link));
                }
                // Check j -> i
                if let Some(link) = Self::calculate_indirect_link(&nodes, j, i) {
                    new_links.push((j, link));
                }
            }
        }

        // Apply calculated links
        for (node_idx, link) in new_links {
            if let Some(node) = nodes.get_mut(node_idx) {
                node.io_links.push(link);
                node.properties.io_links_count += 1;
            }
        }

        system_props.num_nodes = nodes.len() as u32;

        Ok(Self {
            system_props,
            nodes,
        })
    }

    /// Port of `topology_sysfs_get_node_props` logic for properties not found in sysfs.
    fn enrich_gpu_properties(props: &mut HsaNodeProperties) {
        // Bail out if CPU node
        if props.simd_count == 0 {
            return;
        }

        // 1. Decode GFX Target Version (default)
        // KFD reported: 100300 -> 10.3.0
        let mut major = (props.gfx_target_version / 10000) % 100;
        let mut minor = (props.gfx_target_version / 100) % 100;
        let mut step = props.gfx_target_version % 100;

        // 2. Check Overrides (Global and Per-Node)
        let override_var_node = format!("HSA_OVERRIDE_GFX_VERSION_{}", props.node_id);
        let override_val =
            env::var(&override_var_node).or_else(|_| env::var("HSA_OVERRIDE_GFX_VERSION"));

        if let Ok(val) = override_val {
            let parts: Vec<&str> = val.split('.').collect();
            if parts.len() == 3 {
                if let (Ok(maj), Ok(min), Ok(stp)) = (
                    parts[0].parse::<u32>(),
                    parts[1].parse::<u32>(),
                    parts[2].parse::<u32>(),
                ) {
                    // Valid override found, update local vars
                    major = maj;
                    minor = min;
                    step = stp;
                }
            }
        }

        // 3. Update Engine ID
        props.engine_id = EngineId {
            major,
            minor,
            stepping: step,
        };

        // 4. Lookup Marketing Name / AMD Name in table
        if let Some(entry) = find_gfx_ip(props.device_id as u16, major as u8) {
            props.amd_name = entry.name.to_string();
            // If table has stricter versioning, update EngineID
            props.engine_id.major = entry.major as u32;
            props.engine_id.minor = entry.minor as u32;
            props.engine_id.stepping = entry.stepping as u32;
        } else {
            // Default AMD Name
            props.amd_name = format!("GFX{:02x}", props.gfx_target_version);
        }

        // 5. Marketing Name Fallback
        if props.marketing_name.is_empty() {
            // In C this calls DRM. Here we use a generic fallback or the table name.
            props.marketing_name = props.amd_name.clone();
        }

        // 6. Derived Properties
        if props.simd_arrays_per_engine != 0 {
            // C code uses `array_count` (total SIMD arrays) / `simd_arrays_per_engine` (SEs?)
            // Actually `props->NumArrays` in C struct maps to `simd_arrays_per_engine` in sysfs
            props.num_shader_banks = props.array_count / props.simd_arrays_per_engine;
        }

        props.sgpr_size_per_cu = SGPR_SIZE_PER_CU;
        props.vgpr_size_per_cu = get_vgpr_size_per_cu(
            props.engine_id.major,
            props.engine_id.minor,
            props.engine_id.stepping,
        );

        // Fix for missing num_xcc on older kernels
        if props.num_xcc == 0 {
            props.num_xcc = 1;
        }
    }

    /// Port of `get_indirect_iolink_info` from topology.c
    /// Calculates connections for GPU->CPU->GPU, GPU->CPU->CPU->GPU, etc.
    fn calculate_indirect_link(
        nodes: &[Node],
        src_idx: usize,
        dst_idx: usize,
    ) -> Option<HsaIoLinkProperties> {
        let src = &nodes[src_idx];
        let dst = &nodes[dst_idx];

        let src_is_gpu = src.properties.simd_count > 0;
        let dst_is_gpu = dst.properties.simd_count > 0;

        // CPU-CPU links are usually handled by OS/NUMA, KFD only creates GPU links or Indirects
        if !src_is_gpu && !dst_is_gpu {
            return None;
        }

        // Determine the "Direct CPU" for a node.
        // If node is CPU, it is its own direct CPU.
        // If node is GPU, find the PCIe/XGMI link to a CPU.
        let get_direct_cpu = |node: &Node, idx: usize| -> Option<usize> {
            if node.properties.simd_count == 0 {
                return Some(idx); // Is CPU
            }
            // Find link to CPU
            node.io_links
                .iter()
                .find(|l| {
                    let is_direct = l.weight <= 20;
                    let valid_type =
                        l.type_ == HSA_IOLINKTYPE_PCIEXPRESS || l.type_ == HSA_IOLINKTYPE_XGMI;
                    // Check if target is CPU
                    if let Some(target) = nodes.get(l.node_to as usize) {
                        return is_direct && valid_type && target.properties.simd_count == 0;
                    }
                    false
                })
                .map(|l| l.node_to as usize)
        };

        let cpu_src = get_direct_cpu(src, src_idx)?;
        let cpu_dst = get_direct_cpu(dst, dst_idx)?;

        let mut weight1 = 0;
        let mut weight2 = 0;
        let mut weight3 = 0;
        let mut link_type = HSA_IOLINKTYPE_UNDEFINED;

        if cpu_src == cpu_dst {
            // Case 1: GPU -> CPU -> GPU (or GPU->CPU, CPU->GPU)
            // Path: Src -> CPU -> Dst

            if src_is_gpu {
                let l = src
                    .io_links
                    .iter()
                    .find(|l| l.node_to as usize == cpu_src)?;
                weight1 = l.weight;
            }

            if dst_is_gpu {
                // In C code: `get_direct_iolink_info(dir_cpu1, node2...)`
                // Find link from CPU to Dst.
                let l = nodes[cpu_src]
                    .io_links
                    .iter()
                    .find(|l| l.node_to as usize == dst_idx)?;
                weight2 = l.weight;
                link_type = if src_is_gpu {
                    HSA_IOLINKTYPE_PCIEXPRESS
                } else {
                    l.type_
                };
            }
        } else {
            // Case 2: GPU -> CPU1 -> CPU2 -> GPU (Indirect / QPI)
            // Path: Src -> CPU1 -> CPU2 -> Dst

            if src_is_gpu {
                let l = src
                    .io_links
                    .iter()
                    .find(|l| l.node_to as usize == cpu_src)?;
                weight1 = l.weight;
            }

            // CPU1 -> CPU2
            let l_cpu = nodes[cpu_src]
                .io_links
                .iter()
                .find(|l| l.node_to as usize == cpu_dst)?;
            weight2 = l_cpu.weight;

            // C code check: "On QPI... CPU<->CPU weight > 20 means different sockets... not supported"
            if l_cpu.type_ == HSA_IOLINKTYPE_QPI_1_1 && weight2 > 20 {
                return None;
            }

            if dst_is_gpu {
                // CPU2 -> Dst
                let l = nodes[cpu_dst]
                    .io_links
                    .iter()
                    .find(|l| l.node_to as usize == dst_idx)?;
                weight3 = l.weight;
            }
        }

        // C code sums the weights
        let total_weight = weight1 + weight2 + weight3;
        if total_weight == 0 {
            return None;
        }

        Some(HsaIoLinkProperties {
            type_: link_type,
            version_major: 0,
            version_minor: 0,
            node_from: src_idx as u32,
            node_to: dst_idx as u32,
            weight: total_weight,
            min_latency: 0,
            max_latency: 0,
            min_bandwidth: 0,
            max_bandwidth: 0,
            rec_transfer_size: 0,
            rec_sdma_eng_id_mask: 0,
            flags: 0,
        })
    }

    fn parse_cpu_info() -> HashMap<u32, String> {
        let mut map = HashMap::new();
        if let Ok(content) = fs::read_to_string("/proc/cpuinfo") {
            let mut apicid = None;
            let mut name = None;
            for line in content.lines() {
                let mut parts = line.split(':');
                if let (Some(k), Some(v)) = (parts.next(), parts.next()) {
                    let k = k.trim();
                    let v = v.trim();
                    if k == "apicid" || k == "initial apicid" {
                        apicid = v.parse::<u32>().ok();
                    } else if k == "model name" {
                        name = Some(v.to_string());
                    }
                }
                if line.trim().is_empty() {
                    if let (Some(id), Some(n)) = (apicid, name.clone()) {
                        map.insert(id, n);
                    }
                    apicid = None;
                }
            }
            if let (Some(id), Some(n)) = (apicid, name) {
                map.insert(id, n);
            }
        }
        map
    }

    fn parse_system_properties(path: &Path) -> io::Result<HsaSystemProperties> {
        let content = fs::read_to_string(path)?;
        let mut p = HsaSystemProperties::default();
        for line in content.lines() {
            let mut parts = line.split_whitespace();
            if let (Some(k), Some(v)) = (parts.next(), parts.next()) {
                if let Ok(val) = v.parse::<u32>() {
                    match k {
                        "platform_oem" => p.platform_oem = val,
                        "platform_id" => p.platform_id = val,
                        "platform_rev" => p.platform_rev = val,
                        _ => {}
                    }
                }
            }
        }
        Ok(p)
    }
}

// ===============================================================================================
// Node Parsing (Sysfs Traversal)
// ===============================================================================================

impl Node {
    fn from_sysfs(path: &Path) -> io::Result<Self> {
        let mut properties = Self::parse_node_properties(&path.join("properties"))?;

        // Fallback for GPU ID
        if properties.kfd_gpu_id == 0 {
            if let Ok(txt) = fs::read_to_string(path.join("gpu_id")) {
                if let Ok(val) = txt.trim().parse::<u32>() {
                    properties.kfd_gpu_id = val;
                }
            }
        }

        let mem_banks =
            Self::parse_sub_objects(&path.join("mem_banks"), Self::parse_memory_properties)?;
        let caches = Self::parse_sub_objects(&path.join("caches"), Self::parse_cache_properties)?;
        let mut io_links =
            Self::parse_sub_objects(&path.join("io_links"), Self::parse_iolink_properties)?;
        let mut p2p_links =
            Self::parse_sub_objects(&path.join("p2p_links"), Self::parse_iolink_properties)?;
        io_links.append(&mut p2p_links);

        Ok(Self {
            properties,
            mem_banks,
            caches,
            io_links,
        })
    }

    fn parse_node_properties(path: &Path) -> io::Result<HsaNodeProperties> {
        let content = fs::read_to_string(path)?;
        let mut p = HsaNodeProperties::default();

        for line in content.lines() {
            let mut parts = line.split_whitespace();
            let key = parts.next();
            let val_str = parts.next();

            if let (Some(k), Some(v)) = (key, val_str) {
                if let Ok(val) = v.parse::<u64>() {
                    match k {
                        "cpu_cores_count" => p.cpu_cores_count = val as u32,
                        "simd_count" => p.simd_count = val as u32,
                        "mem_banks_count" => p.mem_banks_count = val as u32,
                        "caches_count" => p.caches_count = val as u32,
                        "io_links_count" => p.io_links_count = val as u32,
                        "p2p_links_count" => p.p2p_links_count = val as u32,
                        "cpu_core_id_base" => p.cpu_core_id_base = val as u32,
                        "simd_id_base" => p.simd_id_base = val as u32,
                        "capability" => p.capability = val as u32,
                        "capability2" => p.capability2 = val as u32,
                        "debug_prop" => p.debug_prop = val,
                        "max_waves_per_simd" => p.max_waves_per_simd = val as u32,
                        "lds_size_in_kb" => p.lds_size_in_kb = val as u32,
                        "gds_size_in_kb" => p.gds_size_in_kb = val as u32,
                        "wave_front_size" => p.wave_front_size = val as u32,
                        "array_count" => p.array_count = val as u32,
                        "simd_arrays_per_engine" => p.simd_arrays_per_engine = val as u32,
                        "cu_per_simd_array" => p.cu_per_simd_array = val as u32,
                        "simd_per_cu" => p.simd_per_cu = val as u32,
                        "max_slots_scratch_cu" => p.max_slots_scratch_cu = val as u32,
                        "fw_version" => p.fw_version = val as u32,
                        "vendor_id" => p.vendor_id = val as u32,
                        "device_id" => p.device_id = val as u32,
                        "location_id" => p.location_id = val as u32,
                        "domain" => p.domain = val as u32,
                        "max_engine_clk_fcompute" => p.max_engine_clk_fcompute = val as u32,
                        "max_engine_clk_ccompute" => p.max_engine_clk_ccompute = val as u32,
                        "local_mem_size" => p.local_mem_size = val,
                        "drm_render_minor" => p.drm_render_minor = val as i32,
                        "hive_id" => p.hive_id = val,
                        "unique_id" => p.unique_id = val,
                        "num_sdma_engines" => p.num_sdma_engines = val as u32,
                        "num_sdma_xgmi_engines" => p.num_sdma_xgmi_engines = val as u32,
                        "num_gws" => p.num_gws = val as u32,
                        "num_sdma_queues_per_engine" => p.num_sdma_queues_per_engine = val as u32,
                        "num_cp_queues" => p.num_cp_queues = val as u32,
                        "num_xcc" => p.num_xcc = val as u32,
                        "gfx_target_version" => p.gfx_target_version = val as u32,
                        _ => {}
                    }
                }
                if k == "name" {
                    p.marketing_name = v.to_string();
                }
            }
        }
        Ok(p)
    }

    fn parse_memory_properties(path: &Path) -> io::Result<HsaMemoryProperties> {
        let content = fs::read_to_string(path.join("properties"))?;
        let mut p = HsaMemoryProperties::default();
        for line in content.lines() {
            let mut parts = line.split_whitespace();
            if let (Some(k), Some(v)) = (parts.next(), parts.next()) {
                if let Ok(val) = v.parse::<u64>() {
                    match k {
                        "heap_type" => p.heap_type = val as u32,
                        "size_in_bytes" => p.size_in_bytes = val,
                        "flags" => p.flags = val as u32,
                        "width" => p.width = val as u32,
                        "mem_clk_max" => p.mem_clk_max = val as u32,
                        _ => {}
                    }
                }
            }
        }
        Ok(p)
    }

    fn parse_cache_properties(path: &Path) -> io::Result<HsaCacheProperties> {
        let content = fs::read_to_string(path.join("properties"))?;
        let mut p = HsaCacheProperties::default();
        for line in content.lines() {
            let mut parts = line.split_whitespace();
            let key = parts.next();
            if key == Some("sibling_map") {
                for num_str in parts {
                    let clean = num_str.trim_matches(',');
                    if let Ok(val) = clean.parse::<u32>() {
                        p.sibling_map.push(val);
                    }
                }
                continue;
            }
            if let (Some(k), Some(v)) = (key, parts.next()) {
                if let Ok(val) = v.parse::<u32>() {
                    match k {
                        "processor_id_low" => p.processor_id_low = val,
                        "level" => p.cache_level = val,
                        "size" => p.cache_size = val,
                        "cache_line_size" => p.cache_line_size = val,
                        "cache_lines_per_tag" => p.cache_lines_per_tag = val,
                        "association" => p.cache_associativity = val,
                        "latency" => p.cache_latency = val,
                        "type" => p.cache_type = val,
                        _ => {}
                    }
                }
            }
        }
        Ok(p)
    }

    fn parse_iolink_properties(path: &Path) -> io::Result<HsaIoLinkProperties> {
        let content = fs::read_to_string(path.join("properties"))?;
        let mut p = HsaIoLinkProperties::default();
        for line in content.lines() {
            let mut parts = line.split_whitespace();
            if let (Some(k), Some(v)) = (parts.next(), parts.next()) {
                if let Ok(val) = v.parse::<u32>() {
                    match k {
                        "type" => p.type_ = val,
                        "version_major" => p.version_major = val,
                        "version_minor" => p.version_minor = val,
                        "node_from" => p.node_from = val,
                        "node_to" => p.node_to = val,
                        "weight" => p.weight = val,
                        "min_latency" => p.min_latency = val,
                        "max_latency" => p.max_latency = val,
                        "min_bandwidth" => p.min_bandwidth = val,
                        "max_bandwidth" => p.max_bandwidth = val,
                        "recommended_transfer_size" => p.rec_transfer_size = val,
                        "recommended_sdma_engine_id_mask" => p.rec_sdma_eng_id_mask = val,
                        "flags" => p.flags = val,
                        _ => {}
                    }
                }
            }
        }
        Ok(p)
    }

    fn parse_sub_objects<T, F>(dir: &Path, parse_func: F) -> io::Result<Vec<T>>
    where
        F: Fn(&Path) -> io::Result<T>,
    {
        let mut results = Vec::new();
        if !dir.exists() {
            return Ok(results);
        }
        if let Ok(entries) = fs::read_dir(dir) {
            let mut paths: Vec<_> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
            paths.sort_by_key(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .and_then(|s| s.parse::<u32>().ok())
                    .unwrap_or(u32::MAX)
            });
            for path in paths {
                if path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .and_then(|s| s.parse::<u32>().ok())
                    .is_some()
                {
                    if let Ok(obj) = parse_func(&path) {
                        results.push(obj);
                    }
                }
            }
        }
        Ok(results)
    }
}
