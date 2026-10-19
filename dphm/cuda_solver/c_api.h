// Generated/maintained with AI; see repository disclaimers.md.
#pragma once
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct DphmWorkspace DphmWorkspace;
// One level: arg1=BASE_ARG is a base, arg1=0 a variable, otherwise an arrow.
// The base marker and zero sentinel are not type IDs.
#define DPHM_BASE_ARG UINT32_MAX

typedef struct {
  uint32_t arg1, ret;
} DphmType;

typedef struct {
  uint32_t left, right;
} DphmEquation;

// Resolved unary AST: 0=Var(binder,0), 1=Lam(body,0), 2=App(f,x), 3=Base(1|2,0).
typedef struct {
  uint32_t tag, a, b;
} DphmSourceNode;

typedef struct {
  uint32_t first, second;
} DphmBinderTypes;

typedef struct {
  uint32_t arg, ret;
} DphmArrow;

typedef struct {
  uint32_t type_id, binder_id;
} DphmNodeTyping;

typedef struct {
  double native_ms, allocation_ms;
  uint64_t peak_device_bytes, workspace_bytes;
  uint64_t kernel_launches, primitive_calls, synchronizations;
  uint64_t macros, down_batches, up_batches, occurs_waves;
  double upload_ms, initialize_ms, down_ms, u1_ms, up_ms, bulk_ms, occurs_ms, download_ms;
  uint64_t generated_equations;
  double merge_ms, flatten_ms, class_validation_ms;
  double host_prepare_ms, graph_build_ms;
  uint64_t graph_launches;
  double host_register_ms, host_unregister_ms;
  uint64_t registered_output_bytes;
} DphmMetrics;

// No C++ exception escapes these functions; errors use return codes and a
// NUL-terminated message when capacity>0.
// Return: 0 success; inference also returns 1 invalid, 2 rigid conflict, 3 infinite;
// -1 is a runtime/API failure, not a typing rejection. Discard after CUDA failure.
int dphm_create(DphmWorkspace** out, char* error, size_t capacity);
void dphm_destroy(DphmWorkspace* workspace);

// Last call's device inference interval, excluding initialization/transfers.
// Host-only read; -1 if unavailable or workspace is null.
double dphm_inference_ms(const DphmWorkspace* workspace);

// Whole-tree unary result: arrows[arrows] and source[node_count], normalized through
// final type-bearing representatives. Leaves are implicit: 1/2=Bool/Int,
// 3..(3+variables-1)=variables; arrow ID t indexes arrows[t-(3+variables)].
// source rows retain node and binder types (binder_id=0 when absent).
// Input is a checked closed unary tree; variables=2*lambdas+applications,
// arrows=lambdas+applications. Checker 0=A, 1=B; both use U1/CAS.
// One caller per workspace. Input must stay immutable until return; output
// buffers must be disjoint from the input and each other, and are valid only
// on success. Syntax/scope validation is required, but no prior type inference.
// device_ms includes export; export_ms measures inference-end through export-end.
// register_output=1 temporarily page-locks large outputs; 0 uses ordinary copies.
// Registration/unregistration are inside the call. Unavailable pinning falls back
// to ordinary copies. Buffers remain caller-owned; no registration owned by
// this call remains on return (externally registered memory is left alone).
int dphm_infer_unary_ast(DphmWorkspace*,
    const DphmSourceNode*,
    size_t node_count,
    size_t variables,
    size_t arrows,
    int checker,
    int instrument,
    int register_output,
    DphmArrow*,
    DphmNodeTyping*,
    DphmMetrics*,
    double* lowering_ms,
    double* device_ms,
    double* export_ms,
    char* error,
    size_t capacity);

#ifdef __cplusplus
}
#endif
