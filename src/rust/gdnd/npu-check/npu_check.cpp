/**
 * NPU Check - AscendCL micro-benchmark for NPU health detection
 *
 * Performs a simple memory operation to verify NPU is responsive.
 * This test is designed to:
 * - Be extremely fast (milliseconds)
 * - Detect driver deadlocks that npu-smi cannot see
 * - Have minimal memory footprint
 *
 * Exit codes:
 *   0 - NPU is healthy
 *   1 - AscendCL error occurred
 *   2 - Result verification failed
 *   3 - Timeout or hang detected
 *
 * Requires: CANN Toolkit installed with AscendCL support
 */

#include <acl/acl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <signal.h>
#include <unistd.h>
#include <math.h>

#define MATRIX_SIZE 128
#define DEFAULT_TIMEOUT 5

// Alarm handler for timeout detection
volatile sig_atomic_t timeout_flag = 0;

void timeout_handler(int sig) {
    (void)sig;
    timeout_flag = 1;
}

// Check ACL error and exit on failure
#define ACL_CHECK(call) \
    do { \
        aclError err = call; \
        if (err != ACL_SUCCESS) { \
            fprintf(stderr, "AscendCL error at %s:%d: %d\n", \
                    __FILE__, __LINE__, (int)err); \
            aclFinalize(); \
            exit(1); \
        } \
    } while(0)

// Initialize matrix with simple pattern
void init_matrix(float* mat, int N, float value) {
    for (int i = 0; i < N * N; i++) {
        mat[i] = value;
    }
}

// Verify result (simple check - values should be copied correctly)
int verify_result(float* C, int N, float expected) {
    float tolerance = 0.001f;
    for (int i = 0; i < N * N; i++) {
        if (fabsf(C[i] - expected) > tolerance) {
            fprintf(stderr, "Verification failed at index %d: expected %f, got %f\n",
                    i, expected, C[i]);
            return 0;
        }
    }
    return 1;
}

void print_usage(const char* prog) {
    printf("Usage: %s [-d device_id] [-t timeout_seconds] [-v] [-h] [--pcie-test]\n", prog);
    printf("\nOptions:\n");
    printf("  -d           Device ID to test (default: 0)\n");
    printf("  -t           Timeout in seconds (default: 5)\n");
    printf("  -v           Verbose output\n");
    printf("  -h           Show this help\n");
    printf("  --pcie-test  Run PCIe bandwidth test\n");
}

int run_pcie_test(int device_id, int verbose) {
    // Simple PCIe bandwidth test using memory copy
    size_t test_size = 64 * 1024 * 1024; // 64MB
    void* h_data = nullptr;
    void* d_data = nullptr;

    // Allocate host memory (pinned)
    ACL_CHECK(aclrtMallocHost(&h_data, test_size));

    // Allocate device memory
    ACL_CHECK(aclrtMalloc(&d_data, test_size, ACL_MEM_MALLOC_HUGE_FIRST));

    // Initialize host data
    memset(h_data, 0xAB, test_size);

    // Measure H2D bandwidth
    struct timespec start, end;
    clock_gettime(CLOCK_MONOTONIC, &start);

    ACL_CHECK(aclrtMemcpy(d_data, test_size, h_data, test_size, ACL_MEMCPY_HOST_TO_DEVICE));
    ACL_CHECK(aclrtSynchronizeDevice());

    clock_gettime(CLOCK_MONOTONIC, &end);

    double h2d_time = (end.tv_sec - start.tv_sec) + (end.tv_nsec - start.tv_nsec) / 1e9;
    double h2d_bandwidth = (test_size / (1024.0 * 1024.0 * 1024.0)) / h2d_time;

    // Measure D2H bandwidth
    clock_gettime(CLOCK_MONOTONIC, &start);

    ACL_CHECK(aclrtMemcpy(h_data, test_size, d_data, test_size, ACL_MEMCPY_DEVICE_TO_HOST));
    ACL_CHECK(aclrtSynchronizeDevice());

    clock_gettime(CLOCK_MONOTONIC, &end);

    double d2h_time = (end.tv_sec - start.tv_sec) + (end.tv_nsec - start.tv_nsec) / 1e9;
    double d2h_bandwidth = (test_size / (1024.0 * 1024.0 * 1024.0)) / d2h_time;

    if (verbose) {
        printf("PCIe Bandwidth Test Results:\n");
        printf("  Host to Device: %.2f GB/s\n", h2d_bandwidth);
        printf("  Device to Host: %.2f GB/s\n", d2h_bandwidth);
    }

    // Cleanup
    aclrtFree(d_data);
    aclrtFreeHost(h_data);

    // Check if bandwidth is reasonable (> 1 GB/s for PCIe 3.0+)
    if (h2d_bandwidth < 1.0 || d2h_bandwidth < 1.0) {
        fprintf(stderr, "Warning: Low PCIe bandwidth detected\n");
        return 2;
    }

    return 0;
}

int main(int argc, char** argv) {
    int device_id = 0;
    int timeout_sec = DEFAULT_TIMEOUT;
    int verbose = 0;
    int pcie_test = 0;

    // Parse arguments
    for (int i = 1; i < argc; i++) {
        if (strcmp(argv[i], "-d") == 0 && i + 1 < argc) {
            device_id = atoi(argv[++i]);
        } else if (strcmp(argv[i], "-t") == 0 && i + 1 < argc) {
            timeout_sec = atoi(argv[++i]);
        } else if (strcmp(argv[i], "-v") == 0) {
            verbose = 1;
        } else if (strcmp(argv[i], "-h") == 0) {
            print_usage(argv[0]);
            return 0;
        } else if (strcmp(argv[i], "--pcie-test") == 0) {
            pcie_test = 1;
        }
    }

    // Setup timeout alarm
    signal(SIGALRM, timeout_handler);
    alarm(timeout_sec);

    if (verbose) {
        printf("NPU Check: Testing device %d with %ds timeout\n", device_id, timeout_sec);
    }

    // Initialize AscendCL
    aclError ret = aclInit(nullptr);
    if (ret != ACL_SUCCESS) {
        fprintf(stderr, "Failed to initialize AscendCL: %d\n", (int)ret);
        return 1;
    }

    // Check for timeout
    if (timeout_flag) {
        fprintf(stderr, "Timeout during AscendCL initialization\n");
        aclFinalize();
        return 3;
    }

    // Get device count
    uint32_t device_count = 0;
    ACL_CHECK(aclrtGetDeviceCount(&device_count));

    if ((uint32_t)device_id >= device_count) {
        fprintf(stderr, "Error: Device %d not found (only %u devices available)\n",
                device_id, device_count);
        aclFinalize();
        return 1;
    }

    // Set device
    ACL_CHECK(aclrtSetDevice(device_id));

    if (verbose) {
        // Get device name (if available)
        const char* soc_name = aclrtGetSocName();
        if (soc_name) {
            printf("Device: %s\n", soc_name);
        } else {
            printf("Device: Ascend NPU %d\n", device_id);
        }
    }

    // Create context
    aclrtContext context = nullptr;
    ACL_CHECK(aclrtCreateContext(&context, device_id));

    // Check for timeout
    if (timeout_flag) {
        fprintf(stderr, "Timeout during context creation\n");
        aclrtDestroyContext(context);
        aclrtResetDevice(device_id);
        aclFinalize();
        return 3;
    }

    // Run PCIe test if requested
    if (pcie_test) {
        int pcie_result = run_pcie_test(device_id, verbose);
        aclrtDestroyContext(context);
        aclrtResetDevice(device_id);
        aclFinalize();
        return pcie_result;
    }

    // Create stream
    aclrtStream stream = nullptr;
    ACL_CHECK(aclrtCreateStream(&stream));

    // Allocate host memory
    size_t matrix_bytes = MATRIX_SIZE * MATRIX_SIZE * sizeof(float);
    float* h_A = nullptr;
    float* h_B = nullptr;

    ACL_CHECK(aclrtMallocHost((void**)&h_A, matrix_bytes));
    ACL_CHECK(aclrtMallocHost((void**)&h_B, matrix_bytes));

    // Initialize input matrix
    init_matrix(h_A, MATRIX_SIZE, 1.0f);
    memset(h_B, 0, matrix_bytes);

    // Check for timeout
    if (timeout_flag) {
        fprintf(stderr, "Timeout during host memory setup\n");
        aclrtFreeHost(h_A);
        aclrtFreeHost(h_B);
        aclrtDestroyStream(stream);
        aclrtDestroyContext(context);
        aclrtResetDevice(device_id);
        aclFinalize();
        return 3;
    }

    // Allocate device memory
    void* d_A = nullptr;
    void* d_B = nullptr;

    ACL_CHECK(aclrtMalloc(&d_A, matrix_bytes, ACL_MEM_MALLOC_HUGE_FIRST));
    ACL_CHECK(aclrtMalloc(&d_B, matrix_bytes, ACL_MEM_MALLOC_HUGE_FIRST));

    // Check for timeout
    if (timeout_flag) {
        fprintf(stderr, "Timeout during device memory allocation\n");
        aclrtFree(d_A);
        aclrtFree(d_B);
        aclrtFreeHost(h_A);
        aclrtFreeHost(h_B);
        aclrtDestroyStream(stream);
        aclrtDestroyContext(context);
        aclrtResetDevice(device_id);
        aclFinalize();
        return 3;
    }

    // Copy data to device
    ACL_CHECK(aclrtMemcpyAsync(d_A, matrix_bytes, h_A, matrix_bytes,
                               ACL_MEMCPY_HOST_TO_DEVICE, stream));

    // Simple test: copy from d_A to d_B on device
    // This tests basic device memory operations
    ACL_CHECK(aclrtMemcpyAsync(d_B, matrix_bytes, d_A, matrix_bytes,
                               ACL_MEMCPY_DEVICE_TO_DEVICE, stream));

    // Synchronize stream
    ACL_CHECK(aclrtSynchronizeStream(stream));

    // Check for timeout
    if (timeout_flag) {
        fprintf(stderr, "Timeout during device operations\n");
        aclrtFree(d_A);
        aclrtFree(d_B);
        aclrtFreeHost(h_A);
        aclrtFreeHost(h_B);
        aclrtDestroyStream(stream);
        aclrtDestroyContext(context);
        aclrtResetDevice(device_id);
        aclFinalize();
        return 3;
    }

    // Copy result back
    ACL_CHECK(aclrtMemcpyAsync(h_B, matrix_bytes, d_B, matrix_bytes,
                               ACL_MEMCPY_DEVICE_TO_HOST, stream));
    ACL_CHECK(aclrtSynchronizeStream(stream));

    // Verify result - h_B should equal h_A after copy
    if (!verify_result(h_B, MATRIX_SIZE, 1.0f)) {
        fprintf(stderr, "Result verification failed\n");
        aclrtFree(d_A);
        aclrtFree(d_B);
        aclrtFreeHost(h_A);
        aclrtFreeHost(h_B);
        aclrtDestroyStream(stream);
        aclrtDestroyContext(context);
        aclrtResetDevice(device_id);
        aclFinalize();
        return 2;
    }

    // Cleanup
    aclrtFree(d_A);
    aclrtFree(d_B);
    aclrtFreeHost(h_A);
    aclrtFreeHost(h_B);
    aclrtDestroyStream(stream);
    aclrtDestroyContext(context);
    aclrtResetDevice(device_id);

    // Cancel alarm
    alarm(0);

    // Finalize AscendCL
    aclFinalize();

    if (verbose) {
        printf("NPU check passed successfully\n");
    }

    return 0;
}
