#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <string.h>
#include "hsa/hsa.h"
#include "hsa/hsa_ext_amd.h"

#define CHECK(status) \
    if (status != HSA_STATUS_SUCCESS) { \
        const char* err; \
        hsa_status_string(status, &err); \
        fprintf(stderr, "[-] HSA Error at line %d: %s\n", __LINE__, err); \
        return status; \
    }

typedef struct {
    int node_index;
} agent_iterator_data_t;

hsa_status_t print_cache_info(hsa_cache_t cache, void* data) {
    uint8_t level = 0; 
    uint32_t size = 0;
    
    hsa_cache_get_info(cache, HSA_CACHE_INFO_LEVEL, &level);
    hsa_cache_get_info(cache, HSA_CACHE_INFO_SIZE, &size);
    
    if (size == 0) {
        printf("      L%u Size: Unknown (Reported 0)\n", level);
    } else {
        printf("      L%u Size: %u KB\n", level, size / 1024);
    }
    return HSA_STATUS_SUCCESS;
}

hsa_status_t print_region_info(hsa_region_t region, void* data) {
    hsa_region_segment_t segment;
    size_t size = 0;
    bool host_accessible = false;
    
    hsa_region_get_info(region, HSA_REGION_INFO_SEGMENT, &segment);
    hsa_region_get_info(region, HSA_REGION_INFO_SIZE, &size);
    hsa_region_get_info(region, HSA_AMD_REGION_INFO_HOST_ACCESSIBLE, &host_accessible);

    const char* type_str = "Unknown";
    switch (segment) {
        case HSA_REGION_SEGMENT_GLOBAL:
            type_str = host_accessible ? "System" : "FrameBuffer (VRAM)";
            break;
        case HSA_REGION_SEGMENT_GROUP:
            type_str = "LDS (Group)";
            break;
        case HSA_REGION_SEGMENT_PRIVATE:
            type_str = "Scratch (Private)";
            break;
        case HSA_REGION_SEGMENT_READONLY:
            type_str = "Constant (ReadOnly)";
            break;
        default: break;
    }

    if (segment == HSA_REGION_SEGMENT_GLOBAL) {
        static int mem_idx = 0; 
        printf("      [%d] %-20s Size: %zu MB\n", mem_idx++, type_str, size / 1024 / 1024);
    }
    
    return HSA_STATUS_SUCCESS;
}

hsa_status_t print_agent_info(hsa_agent_t agent, void* data) {
    agent_iterator_data_t* iter_data = (agent_iterator_data_t*)data;
    
    char name[64] = {0};
    char product_name[64] = {0};
    hsa_device_type_t device_type;
    uint32_t node_id = 0;

    hsa_agent_get_info(agent, HSA_AGENT_INFO_NAME, name);
    hsa_agent_get_info(agent, HSA_AGENT_INFO_DEVICE, &device_type);
    hsa_agent_get_info(agent, HSA_AMD_AGENT_INFO_PRODUCT_NAME, product_name);
    hsa_agent_get_info(agent, HSA_AMD_AGENT_INFO_DRIVER_NODE_ID, &node_id);

    printf("\n------------------------------------------------------------\n");
    printf(" Node %u (%s)\n", node_id, (product_name[0] != '\0') ? product_name : name);
    printf("------------------------------------------------------------\n");

    if (device_type == HSA_DEVICE_TYPE_GPU) {
        uint32_t compute_units = 0;
        uint32_t simds_per_cu = 0;
        uint32_t max_waves_per_cu = 0;
        uint32_t bdf_id = 0;
        uint32_t domain_id = 0;
        uint32_t chip_id = 0;
        
        hsa_agent_get_info(agent, HSA_AMD_AGENT_INFO_COMPUTE_UNIT_COUNT, &compute_units);
        hsa_agent_get_info(agent, HSA_AMD_AGENT_INFO_NUM_SIMDS_PER_CU, &simds_per_cu);
        hsa_agent_get_info(agent, HSA_AMD_AGENT_INFO_MAX_WAVES_PER_CU, &max_waves_per_cu);
        hsa_agent_get_info(agent, HSA_AMD_AGENT_INFO_BDFID, &bdf_id);
        hsa_agent_get_info(agent, HSA_AMD_AGENT_INFO_DOMAIN, &domain_id);
        hsa_agent_get_info(agent, HSA_AMD_AGENT_INFO_CHIP_ID, &chip_id);

        uint32_t total_simds = compute_units * simds_per_cu;
        uint32_t waves_per_simd = (simds_per_cu > 0) ? max_waves_per_cu / simds_per_cu : 0;

        printf("    Type:          GPU\n");
        printf("    Compute Units: %u\n", compute_units);
        printf("    SIMDs:         %u\n", total_simds);
        printf("    Waves/SIMD:    %u\n", waves_per_simd);
        printf("    Chip ID:       0x%x\n", chip_id);
        printf("    Location ID:   0x%x (Domain: %u)\n", bdf_id, domain_id);
        
    } else if (device_type == HSA_DEVICE_TYPE_CPU) {
        printf("    Type:          CPU\n");
    } else {
        printf("    Type:          Other\n");
    }

    printf("\n    Memory Banks:\n");
    hsa_agent_iterate_regions(agent, print_region_info, NULL);

    printf("\n    Caches:\n");
    hsa_agent_iterate_caches(agent, print_cache_info, NULL);

    iter_data->node_index++;
    return HSA_STATUS_SUCCESS;
}

int main() {
    printf("============================================================\n");
    printf("             HSA Runtime (C) - Diagnostics                  \n");
    printf("============================================================\n");

    printf("[+] Initializing HSA Runtime...\n");
    hsa_status_t status = hsa_init();
    if (status != HSA_STATUS_SUCCESS) {
        const char* err_str;
        hsa_status_string(status, &err_str);
        fprintf(stderr, "[-] HSA failed to initialize: %s\n", err_str);
        return 1;
    }

    uint16_t major_ver, minor_ver;
    CHECK(hsa_system_get_info(HSA_SYSTEM_INFO_VERSION_MAJOR, &major_ver));
    CHECK(hsa_system_get_info(HSA_SYSTEM_INFO_VERSION_MINOR, &minor_ver));
    printf("[+] HSA Interface Version: %u.%u\n", major_ver, minor_ver);

    printf("\n[+] Scanning System Agents...\n");
    
    agent_iterator_data_t iter_data = {0};
    status = hsa_iterate_agents(print_agent_info, &iter_data);
    CHECK(status);

    printf("\n[+] Diagnostics Complete.\n");

    CHECK(hsa_shut_down());

    return 0;
}
