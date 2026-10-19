// Generated/maintained with AI; see repository disclaimers.md.
#pragma once
#include "c_api.h"
#include <cuda_runtime.h>

namespace dphm::cuda_solver {

// All 32 lanes participate. Reserve exactly the live pair count, then scatter
// into disjoint locations. The limit is relative to output, including any offset.
__device__ inline void append_pairs(
    DphmEquation a, DphmEquation b, DphmEquation* output, int limit, int* count, int* status) {
  const unsigned lane = threadIdx.x & 31;
  const unsigned first = __ballot_sync(0xffffffffu, a.left != a.right);
  const unsigned second = __ballot_sync(0xffffffffu, b.left != b.right);
  const unsigned before = (1u << lane) - 1;
  int offset = __popc(first & before) + __popc(second & before);
  const int total = __popc(first) + __popc(second);

  int base = lane == 0 && total ? atomicAdd(count, total) : 0;
  base = __shfl_sync(0xffffffffu, base, 0);

  // Warp-uniform failure: no lane writes a partial or out-of-bounds reservation.
  // The caller must reject this batch before consuming the reservation counter.
  if (base < 0 || total > limit || base > limit - total) {
    if (lane == 0)
      atomicMax(status, 4);
    return;
  }

  if (a.left != a.right)
    output[base + offset++] = a;
  if (b.left != b.right)
    output[base + offset++] = b;
}

}  // namespace dphm::cuda_solver
