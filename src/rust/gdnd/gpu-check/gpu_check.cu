/**
 * GPU Check - CUDA micro-benchmark for GPU health detection
 *
 * Performs a small matrix multiplication (128x128) to verify GPU is responsive.
 * This test is designed to:
 * - Be extremely fast (milliseconds)
 * - Detect driver deadlocks that nvidia-smi cannot see
 * - Have minimal memory footprint
 *
 * Exit codes:
 *   0 - GPU is healthy
 *   1 - CUDA error occurred
 *   2 - Result verification failed
 *   3 - Timeout or hang detected
 */

#include <cuda_runtime.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <signal.h>
#include <unistd.h>

#define MATRIX_SIZE 128
#define BLOCK_SIZE 16
#define DEFAULT_TIMEOUT 5

// Alarm handler for timeout detection
volatile sig_atomic_t timeout_flag = 0;

void timeout_handler(int sig) {
    timeout_flag = 1;
}

// Check CUDA error and exit on failure
#define CUDA_CHECK(call) \
    do { \
        cudaError_t err = call; \
        if (err != cudaSuccess) { \
            fprintf(stderr, "CUDA error at %s:%d: %s\n", \
                    __FILE__, __LINE__, cudaGetErrorString(err)); \
            exit(1); \
        } \
    } while(0)

// Simple matrix multiplication kernel
__global__ void matmul_kernel(float* A, float* B, float* C, int N) {
    int row = blockIdx.y * blockDim.y + threadIdx.y;
    int col = blockIdx.x * blockDim.x + threadIdx.x;

    if (row < N && col < N) {
        float sum = 0.0f;
        for (int k = 0; k < N; k++) {
            sum += A[row * N + k] * B[k * N + col];
        }
        C[row * N + col] = sum;
    }
}

// Initialize matrix with simple pattern
void init_matrix(float* mat, int N, float value) {
    for (int i = 0; i < N * N; i++) {
        mat[i] = value;
    }
}

// Verify result (all values should be N * value^2 for uniform input)
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
    printf("Usage: %s [-d device_id] [-t timeout_seconds] [-v] [-h]\n", prog);
    printf("\nOptions:\n");
    printf("  -d  Device ID to test (default: 0)\n");
    printf("  -t  Timeout in seconds (default: 5)\n");
    printf("  -v  Verbose output\n");
    printf("  -h  Show this help\n");
}

int main(int argc, char** argv) {
    int device_id = 0;
    int timeout_sec = DEFAULT_TIMEOUT;
    int verbose = 0;

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
        }
    }

    // Setup timeout alarm
    signal(SIGALRM, timeout_handler);
    alarm(timeout_sec);

    if (verbose) {
        printf("GPU Check: Testing device %d with %ds timeout\n", device_id, timeout_sec);
    }

    // Get device count
    int device_count;
    CUDA_CHECK(cudaGetDeviceCount(&device_count));

    if (device_id >= device_count) {
        fprintf(stderr, "Error: Device %d not found (only %d devices available)\n",
                device_id, device_count);
        return 1;
    }

    // Set device
    CUDA_CHECK(cudaSetDevice(device_id));

    // Get device properties
    cudaDeviceProp prop;
    CUDA_CHECK(cudaGetDeviceProperties(&prop, device_id));

    if (verbose) {
        printf("Device: %s\n", prop.name);
        printf("Compute capability: %d.%d\n", prop.major, prop.minor);
    }

    // Check for timeout
    if (timeout_flag) {
        fprintf(stderr, "Timeout during initialization\n");
        return 3;
    }

    // Allocate host memory
    size_t matrix_bytes = MATRIX_SIZE * MATRIX_SIZE * sizeof(float);
    float* h_A = (float*)malloc(matrix_bytes);
    float* h_B = (float*)malloc(matrix_bytes);
    float* h_C = (float*)malloc(matrix_bytes);

    if (!h_A || !h_B || !h_C) {
        fprintf(stderr, "Failed to allocate host memory\n");
        return 1;
    }

    // Initialize matrices
    init_matrix(h_A, MATRIX_SIZE, 1.0f);
    init_matrix(h_B, MATRIX_SIZE, 1.0f);
    memset(h_C, 0, matrix_bytes);

    // Check for timeout
    if (timeout_flag) {
        fprintf(stderr, "Timeout during host memory setup\n");
        free(h_A); free(h_B); free(h_C);
        return 3;
    }

    // Allocate device memory
    float *d_A, *d_B, *d_C;
    CUDA_CHECK(cudaMalloc(&d_A, matrix_bytes));
    CUDA_CHECK(cudaMalloc(&d_B, matrix_bytes));
    CUDA_CHECK(cudaMalloc(&d_C, matrix_bytes));

    // Check for timeout
    if (timeout_flag) {
        fprintf(stderr, "Timeout during device memory allocation\n");
        cudaFree(d_A); cudaFree(d_B); cudaFree(d_C);
        free(h_A); free(h_B); free(h_C);
        return 3;
    }

    // Copy data to device
    CUDA_CHECK(cudaMemcpy(d_A, h_A, matrix_bytes, cudaMemcpyHostToDevice));
    CUDA_CHECK(cudaMemcpy(d_B, h_B, matrix_bytes, cudaMemcpyHostToDevice));

    // Check for timeout
    if (timeout_flag) {
        fprintf(stderr, "Timeout during memory copy to device\n");
        cudaFree(d_A); cudaFree(d_B); cudaFree(d_C);
        free(h_A); free(h_B); free(h_C);
        return 3;
    }

    // Launch kernel
    dim3 block(BLOCK_SIZE, BLOCK_SIZE);
    dim3 grid((MATRIX_SIZE + BLOCK_SIZE - 1) / BLOCK_SIZE,
              (MATRIX_SIZE + BLOCK_SIZE - 1) / BLOCK_SIZE);

    if (verbose) {
        printf("Launching kernel: grid(%d,%d), block(%d,%d)\n",
               grid.x, grid.y, block.x, block.y);
    }

    matmul_kernel<<<grid, block>>>(d_A, d_B, d_C, MATRIX_SIZE);
    CUDA_CHECK(cudaGetLastError());

    // Synchronize and check for errors
    CUDA_CHECK(cudaDeviceSynchronize());

    // Check for timeout
    if (timeout_flag) {
        fprintf(stderr, "Timeout during kernel execution\n");
        cudaFree(d_A); cudaFree(d_B); cudaFree(d_C);
        free(h_A); free(h_B); free(h_C);
        return 3;
    }

    // Copy result back
    CUDA_CHECK(cudaMemcpy(h_C, d_C, matrix_bytes, cudaMemcpyDeviceToHost));

    // Verify result
    // For 128x128 matrix of 1.0f, each element should be 128.0f
    float expected = (float)MATRIX_SIZE;
    if (!verify_result(h_C, MATRIX_SIZE, expected)) {
        fprintf(stderr, "Result verification failed\n");
        cudaFree(d_A); cudaFree(d_B); cudaFree(d_C);
        free(h_A); free(h_B); free(h_C);
        return 2;
    }

    // Cleanup
    cudaFree(d_A);
    cudaFree(d_B);
    cudaFree(d_C);
    free(h_A);
    free(h_B);
    free(h_C);

    // Cancel alarm
    alarm(0);

    if (verbose) {
        printf("GPU check passed successfully\n");
    }

    return 0;
}
