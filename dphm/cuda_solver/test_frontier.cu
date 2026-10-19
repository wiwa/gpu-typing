// Generated/maintained with AI; see repository disclaimers.md.
// Exercise the actual warp appender, including overflow without device faults.
#include "frontier.cuh"
#include <algorithm>
#include <array>
#include <cassert>
#include <cstdio>
#include <cstdlib>

#ifdef NDEBUG
#error "Frontier checks require assertions"
#endif

namespace {
void checked(cudaError_t error) {
  if (error != cudaSuccess) {
    std::fprintf(stderr, "%s\n", cudaGetErrorString(error));
    std::exit(1);
  }
}

__global__ void emit(DphmEquation* output, int limit, int* counters, bool empty) {
  const DphmEquation a = empty ? DphmEquation{0, 0} : DphmEquation{1, 2};
  dphm::cuda_solver::append_pairs(a, {3, empty ? 3u : 4u}, output, limit, counters, counters + 1);
}

void check(int prefix, int limit, int initial = 0, int warps = 1, bool empty = false) {
  DphmEquation* output = nullptr;
  int* counters = nullptr;
  std::array<DphmEquation, 512> result{};
  std::array<int, 2> state{initial, 0};

  checked(cudaMalloc(&output, sizeof(result)));
  checked(cudaMalloc(&counters, sizeof(state)));
  checked(cudaMemset(output, 0, sizeof(result)));
  checked(cudaMemcpy(counters, state.data(), sizeof(state), cudaMemcpyHostToDevice));

  emit<<<1, 32 * warps>>>(output + prefix, limit, counters, empty);
  checked(cudaGetLastError());

  checked(cudaMemcpy(result.data(), output, sizeof(result), cudaMemcpyDeviceToHost));
  checked(cudaMemcpy(state.data(), counters, sizeof(state), cudaMemcpyDeviceToHost));

  const int admitted = empty ? 0 : std::min(warps, (limit - initial) / 64);
  assert(state[0] == initial + (empty ? 0 : 64 * warps));
  assert(state[1] == (!empty && admitted < warps ? 4 : 0));
  for (int i = 0; i < static_cast<int>(result.size()); ++i) {
    const bool written = i >= prefix + initial && i < prefix + initial + 64 * admitted;
    assert((result[i].left != 0) == written);
    if (written)
      assert(result[i].right == result[i].left + 1);
  }

  checked(cudaFree(counters));
  checked(cudaFree(output));
}
}  // namespace

int main() {
  check(0, 64);   // Exact capacity, no slack.
  check(0, 63);   // The whole warp skips stores on overflow.
  check(16, 64);  // U1-style output offset, independent append counter.
  check(16, 63);
  check(0, 80, 16);  // A previous warp has already reserved a prefix.
  check(0, 79, 16);
  check(0, 128, 0, 4);  // Racing warp reservations; two fit and two overflow.
  check(0, 0, 0, 1, true);
}
