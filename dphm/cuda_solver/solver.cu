// Generated/maintained with AI; see repository disclaimers.md.
#include "solver.hpp"
#include "frontier.cuh"

#include <cub/cub.cuh>
#include <cuco/bucket_storage.cuh>
#include <cuco/static_set_ref.cuh>
#include <cuda/atomic>
#include <cuda_runtime.h>

#include <algorithm>
#include <array>
#include <chrono>
#include <climits>
#include <limits>
#include <stdexcept>
#include <vector>

namespace dphm::cuda_solver {
namespace {

using Clock = std::chrono::steady_clock;

double milliseconds(Clock::time_point start) {
  return std::chrono::duration<double, std::milli>(Clock::now() - start).count();
}

void checked(cudaError_t error) {
  if (error != cudaSuccess)
    throw std::runtime_error(
        std::string(cudaGetErrorName(error)) + ": " + cudaGetErrorString(error));
}

constexpr int threads = 256;
static_assert(threads % 32 == 0);  // Full-mask warp collectives below.
using AtomicParent = ::cuda::atomic_ref<TypeId, ::cuda::thread_scope_device>;

__host__ __device__ bool is_arrow(TInfo t) {
  return t.arg1 != 0 && t.arg1 != DPHM_BASE_ARG;
}

// Store only an arrow ID. Its frozen children define the hash-table key.
struct FrozenSignatures {
  const TInfo* arena;
  const TypeId* parents;

  struct Key {
    TypeId arg1, reserved, ret;
    __device__ bool operator==(const Key&) const = default;
  };

  __device__ Key key(TypeId id) const {
    const auto t = arena[id];
    return {parents[t.arg1], 0, parents[t.ret]};
  }

  __device__ std::uint32_t operator()(TypeId id) const {
    return cuco::murmurhash3_32<Key>{}(key(id));
  }

  __device__ bool operator()(TypeId a, TypeId b) const {
    return key(a) == key(b);
  }
};

using SignatureStorage = cuco::bucket_storage_ref<TypeId, 1>;
using SignatureProbing = cuco::linear_probing<1, FrozenSignatures>;
using SignatureSet = cuco::static_set_ref<TypeId,
    ::cuda::thread_scope_device,
    FrozenSignatures,
    SignatureProbing,
    SignatureStorage,
    cuco::op::insert_and_find_tag>;
static_assert(AtomicParent::required_alignment <= alignof(TypeId));

unsigned blocks(int count) {
  return static_cast<unsigned>(
      std::max<std::uint64_t>(1, (static_cast<std::uint64_t>(count) + threads - 1) / threads));
}

__device__ int index() {
  return blockIdx.x * blockDim.x + threadIdx.x;
}

__device__ int stride() {
  return gridDim.x * blockDim.x;
}

#include "ast_lowering.cuh"

// Source lowering reserves contiguous base, variable and arrow ranges.
__device__ bool preferred_in_ranges(TypeId a, TypeId b, int bases, int first_fn) {
  // clang-format off
  const bool av = a > bases && a < first_fn, bv = b > bases && b < first_fn;
  // clang-format on
  return av != bv ? !av : a < b;
}

enum Phase {
  initialize_phase,
  down_phase,
  u1_phase,
  up_phase,
  merge_phase,
  flatten_phase,
  validation_phase,
  occurs_phase,
  phase_count
};

constexpr const char* phase_names[] = {
    "initialize", "down", "u1", "up", "merge", "flatten", "class_validation", "occurs"};

struct State {
  int status = 0;  // Public status codes; 4 denotes an internal invariant failure.
  int count = 0, down_count = 0, arrow_count = 0, changed = 0, peel_flag = 0, parity = 0;
  int extra_count = 0;       // A/U1 children; its original frontier count stays frozen.
  int request_capacity = 0;  // Actual allocated request buffer, not a count estimate.
  int first_fn = 0;          // Source lowering places every arrow in this final ID range.
  std::uint64_t kernels = 0, primitives = 0, macros = 0, down_batches = 0;
  std::uint64_t up_batches = 0, occurs_waves = 0, generated = 0;
  std::uint64_t stamp = 0, nanoseconds[phase_count] = {};
};

__device__ void count_kernel(State* s) {
  if (blockIdx.x == 0 && threadIdx.x == 0)
    ++s->kernels;
}

__device__ std::uint64_t timer() {
  std::uint64_t value;
  asm volatile("mov.u64 %0, %%globaltimer;" : "=l"(value));
  return value;
}

__global__ void timestamp(State* s, Phase phase, bool begin) {
  count_kernel(s);

  if (begin)
    s->stamp = timer();
  else
    s->nanoseconds[phase] += timer() - s->stamp;
}

__global__ void reset(State* s, int count, int rows, int request_capacity) {
  *s = {};
  s->count = count;
  s->kernels = 1;
  s->first_fn = rows;
  s->request_capacity = request_capacity;
}

// Successful warps issue no status atomics; errors need no shared-memory barrier.
__device__ void report(State* s, int error) {
  const int maximum = __reduce_max_sync(0xffffffffu, error);
  if ((threadIdx.x & 31) == 0 && maximum)
    atomicMax(&s->status, maximum);
}

__global__ void initialize(const TInfo* arena,
    TypeId* parents,
    TypeId* arrows,
    int rows,
    int bases,
    int arrow_capacity,
    const Equation* eqs,
    int count,
    State* s) {
  count_kernel(s);

  int error = 0;
  int first_fn = rows;

  for (int tile = blockIdx.x * threads; tile < rows; tile += stride()) {
    const int i = tile + threadIdx.x;
    bool arrow = false;

    if (i < rows) {
      parents[i] = i;
      const auto t = arena[i];
      bool valid;
      if (i == 0)
        valid = t.arg1 == 0 && t.ret == 0;
      else if (i <= bases)
        valid = t.arg1 == DPHM_BASE_ARG && t.ret == i;
      else {
        valid = (t.arg1 == 0 && t.ret == i) ||
                (is_arrow(t) && t.arg1 < rows && t.ret > 0 && t.ret < rows);
      }
      error |= !valid;
      arrow = i != 0 && is_arrow(t);
      if (arrow)
        first_fn = min(first_fn, i);
    }

    // Build the compact arrow list while already visiting input records.
    const unsigned lane = threadIdx.x & 31;
    const unsigned mask = __ballot_sync(0xffffffffu, arrow);
    int base = lane == 0 && mask ? atomicAdd(&s->arrow_count, __popc(mask)) : 0;
    base = __shfl_sync(0xffffffffu, base, 0);

    if (arrow) {
      const int slot = base + __popc(mask & ((1u << lane) - 1));
      if (slot < arrow_capacity)
        arrows[slot] = i;
      else
        error |= 1;  // Reject before any consumer uses this incomplete list.
    }
  }

  const int warp_first = __reduce_min_sync(0xffffffffu, static_cast<unsigned>(first_fn));
  if ((threadIdx.x & 31) == 0 && warp_first < rows)
    atomicMin(&s->first_fn, warp_first);

  for (std::uint64_t i = index(); i < static_cast<std::uint64_t>(count); i += stride()) {
    const auto [a, b] = eqs[i];
    error |= a == 0 || a >= rows || b == 0 || b >= rows;
  }

  report(s, error);
}

// Failure replaces one endpoint by its parent; priority strictly improves.
// Parent reads stay atomic during concurrent linking and path halving.
__global__ void merge(
    TypeId* parents, const Equation* eqs, State* s, int bases, bool down_batch = false) {
  count_kernel(s);

  // A's publication handle is set before expansion; overflow can fail afterward.
  // Do not consume its counter (or any partial batch) once that failure is known.
  if (s->status)
    return;

  const int first_fn = s->first_fn;
  const int count = s->count + (down_batch ? s->extra_count : 0);
  if (down_batch && blockIdx.x == 0 && threadIdx.x == 0)
    s->generated += count;

  for (std::uint64_t i = index(); i < static_cast<std::uint64_t>(count); i += stride()) {
    auto [a, b] = eqs[i];
    while (a != b) {
      if (preferred_in_ranges(b, a, bases, first_fn)) {
        const auto t = a;
        a = b;
        b = t;
      }

      auto old = AtomicParent{parents[b]}.load(::cuda::memory_order_relaxed);
      if (old != b) {
        const auto next = AtomicParent{parents[old]}.load(::cuda::memory_order_relaxed);
        if (next != old)
          atomicCAS(parents + b, old, next);
        b = next;
        continue;
      }

      old = atomicCAS(parents + b, b, a);
      if (old == b)
        break;

      b = old;
    }
  }
}

__global__ void flatten(TypeId* parents, int rows, State* s) {
  count_kernel(s);

  if (s->status || !s->count)
    return;

  for (int i = index(); i < rows; i += stride()) {
    TypeId r = i;
    for (;;) {
      const auto p = AtomicParent{parents[r]}.load(::cuda::memory_order_relaxed);
      if (p == r)
        break;
      r = p;
    }

    if (r != i)
      AtomicParent{parents[i]}.store(r, ::cuda::memory_order_relaxed);
  }
}

__global__ void validate_classes(const TInfo* arena, const TypeId* parents, int rows, State* s) {
  count_kernel(s);

  int error = 0;
  for (int i = index(); i < rows; i += stride()) {
    const auto r = parents[i];
    if (r >= rows || (i != 0 && r == 0)) {
      error = 4;
      continue;
    }
    if (i == 0) {
      if (r != 0)
        error = 4;
      continue;
    }
    const auto t = arena[i], root = arena[r];
    if ((t.arg1 == DPHM_BASE_ARG && r != i) || (is_arrow(t) && root.arg1 == DPHM_BASE_ARG))
      error = max(error, 2);
    if (is_arrow(t) && root.arg1 == 0)
      error = 4;
  }
  report(s, error);
}

__global__ void down(
    const TInfo* arena, const TypeId* parents, const TypeId* arrows, Equation* output, State* s) {
  count_kernel(s);

  const int count = s->arrow_count;
  for (int tile = blockIdx.x * threads; tile < count; tile += stride()) {
    const int j = tile + threadIdx.x;
    Equation a{0, 0}, b{0, 0};
    if (j < count) {
      const int i = arrows[j];
      if (parents[i] != i) {
        const auto t = arena[i], root = arena[parents[i]];
        a = {parents[t.arg1], parents[root.arg1]};
        b = {parents[t.ret], parents[root.ret]};
      }
    }
    append_pairs(a, b, output, s->request_capacity, &s->count, &s->status);
  }
}

// Check rigid compatibility before decomposing. Variables impose no head rule.
__global__ void validate_down(const TInfo* arena,
    const TypeId* parents,
    const TypeId* arrows,
    int rows,
    int bases,
    Equation* output,
    State* s) {
  count_kernel(s);

  int error = 0;
  const int count = bases + s->arrow_count;
  for (int tile = blockIdx.x * threads; tile < count; tile += stride()) {
    const int j = tile + threadIdx.x;
    Equation a{0, 0}, b{0, 0};
    if (j < count) {
      const int i = j < bases ? j + 1 : arrows[j - bases];
      const auto r = parents[i];
      if (r >= rows || (i != 0 && r == 0) || (i == 0 && r != 0))
        error = 4;
      else if (i != 0) {
        const auto t = arena[i];
        if (t.arg1 == DPHM_BASE_ARG && r != i)
          error = max(error, 2);
        else if (is_arrow(t) && r != i) {
          const auto root = arena[r];
          if (root.arg1 == 0)
            error = 4;
          else if (!is_arrow(root))
            error = max(error, 2);
          else {
            a = {parents[t.arg1], parents[root.arg1]};
            b = {parents[t.ret], parents[root.ret]};
          }
        }
      }
    }

    // Rejected and padding lanes still participate in the warp collectives.
    append_pairs(a, b, output, s->request_capacity, &s->count, &s->status);
  }

  report(s, error);
}

// The original Down prefix is immutable. Children append strictly beyond it;
// the graph does not merge anything until this entire frozen layer completes.
template <bool control_loop = false>
__global__ void expand(const TInfo* arena,
    const TypeId* parents,
    Equation* requests,
    State* s,
    cudaGraphConditionalHandle repeat = 0,
    cudaGraphConditionalHandle publish = 0) {
  count_kernel(s);

  const int count = control_loop ? s->count : s->down_count;

  if constexpr (control_loop) {
    // Another warp may report overflow during this kernel. Load atomically and
    // broadcast one decision per full warp before any lane can return.
    int status = 0;
    if ((threadIdx.x & 31) == 0)
      status = ::cuda::atomic_ref<int, ::cuda::thread_scope_device>{s->status}.load(
          ::cuda::memory_order_relaxed);
    status = __shfl_sync(0xffffffffu, status, 0);

    const bool run = status == 0 && count != 0;
    if (blockIdx.x == 0 && threadIdx.x == 0) {
      if (run)
        ++s->down_batches;
      cudaGraphSetConditional(repeat, run);
      cudaGraphSetConditional(publish, run);
    }

    if (!run)
      return;
  }

  int error = 0;
  for (int tile = blockIdx.x * threads; tile < count; tile += stride()) {
    const int i = tile + threadIdx.x;
    Equation a{0, 0}, b{0, 0};
    if (i < count) {
      const auto [left_id, right_id] = requests[i];
      const auto left = arena[left_id], right = arena[right_id];
      if constexpr (!control_loop) {
        if (left.arg1 != 0 && right.arg1 != 0 &&
            (left.arg1 == DPHM_BASE_ARG || right.arg1 == DPHM_BASE_ARG))
          error = 2;
      }
      if (is_arrow(left) && is_arrow(right)) {
        a = {parents[left.arg1], parents[right.arg1]};
        b = {parents[left.ret], parents[right.ret]};
      }
    }

    // Late blocks must see the same input count, not consume appended children.
    const int prefix = control_loop ? count : 0;
    append_pairs(a,
        b,
        requests + prefix,
        s->request_capacity - prefix,
        control_loop ? &s->extra_count : &s->count,
        &s->status);
  }

  // A/U1 leaves rigid conflicts to the next mandatory Down.
  if constexpr (!control_loop)
    report(s, error);
}

__global__ void clear_signatures(TypeId* entries, int capacity, State* s) {
  count_kernel(s);

  for (int i = index(); i < capacity; i += stride())
    entries[i] = 0;
}

// Parents stay frozen. First insertion chooses a group's anchor; every other
// class requests a merge with it. cuCollections publishes one immutable ID.
__global__ void hash_up(const TypeId* parents,
    const TypeId* arrows,
    SignatureSet signatures,
    Equation* output,
    State* s) {
  count_kernel(s);

  const int count = s->arrow_count;
  for (int tile = blockIdx.x * threads; tile < count; tile += stride()) {
    const int i = tile + threadIdx.x;
    Equation e{0, 0};
    if (i < count) {
      const auto anchor = *signatures.insert_and_find(arrows[i]).first;
      e = {parents[anchor], parents[arrows[i]]};
    }
    append_pairs(e, {0, 0}, output, s->request_capacity, &s->count, &s->status);
  }
}

__global__ void seed_finished(const TInfo* arena,
    const TypeId* parents,
    int rows,
    unsigned char* a,
    unsigned char* b,
    State* s) {
  count_kernel(s);

  for (int i = index(); i < rows; i += stride())
    a[i] = b[i] = i == 0 || parents[i] != i || !is_arrow(arena[i]);
}

__global__ void peel(const TInfo* arena,
    const TypeId* parents,
    const TypeId* arrows,
    unsigned char* a,
    unsigned char* b,
    State* s) {
  count_kernel(s);

  const int count = s->arrow_count;
  const auto* old = s->parity ? b : a;
  auto* next = s->parity ? a : b;
  int flag = 0;

  for (int j = index(); j < count; j += stride()) {
    const int i = arrows[j];
    const auto t = arena[i];
    const bool ready = old[i] || (old[parents[t.arg1]] && old[parents[t.ret]]);
    next[i] = ready;
    flag = max(flag, ready && !old[i] ? 2 : (ready ? 0 : 1));
  }

  const int maximum = __reduce_max_sync(0xffffffffu, flag);
  if ((threadIdx.x & 31) == 0 && maximum)
    atomicMax(&s->peel_flag, maximum);
}

enum class Step {
  valid,
  macro_begin,
  down_ready,
  down_close,
  up_begin,
  up_ready,
  macro_end,
  occurs_begin,
  peel_begin,
  peel_end
};

__global__ void control(State* s, Step step, cudaGraphConditionalHandle handle) {
  count_kernel(s);

  bool run = s->status == 0;

  switch (step) {
  case Step::valid:
    break;

  case Step::macro_begin:
    ++s->macros;
    s->count = 0;
    s->extra_count = 0;
    s->changed = 0;
    break;

  case Step::down_ready:
    s->down_count = s->count;
    s->changed = s->count != 0;
    if (s->changed)
      ++s->down_batches;
    run &= s->changed;
    break;

  case Step::down_close:
    if (run)
      s->generated += s->count;
    run &= s->count != 0;
    break;

  case Step::up_begin:
    s->count = 0;
    break;

  case Step::up_ready:
    if (s->count) {
      ++s->up_batches;
      s->generated += s->count;
      s->changed = 1;
    }
    run &= s->count != 0;
    break;

  case Step::macro_end:
    run &= s->changed;
    break;

  case Step::occurs_begin:
    s->parity = 0;
    break;

  case Step::peel_begin:
    ++s->occurs_waves;
    s->peel_flag = 0;
    break;

  case Step::peel_end:
    s->parity ^= 1;
    if (s->peel_flag == 1)
      s->status = 3;
    run = s->peel_flag == 2;
    break;
  }

  if (handle)
    cudaGraphSetConditional(handle, run);
}

struct Memory {
  std::size_t current = 0, peak = 0;
};

// Registration borrows ordinary caller-owned memory; it never changes its allocator.
// No DMA is queued until both registrations succeed or the pageable fallback wins.
struct OutputRegistration {
  std::array<void*, 2> pointers{};

  ~OutputRegistration() {
    for (auto p : pointers)
      if (p)
        cudaHostUnregister(p);
  }

  void release() {
    for (auto& p : pointers)
      if (p) {
        checked(cudaHostUnregister(p));
        p = nullptr;
      }
  }

  std::size_t acquire(const TypingOutput& output) {
    const std::array<std::span<std::byte>, 2> buffers{
        std::as_writable_bytes(output.arrows), std::as_writable_bytes(output.source)};
    const auto total = buffers[0].size() + buffers[1].size();

    // Tiny results do not amortize registration; use ordinary pageable copies.
    if (total < 1024 * 1024)
      return 0;

    for (std::size_t i = 0; i < buffers.size(); ++i) {
      if (buffers[i].empty())
        continue;

      const auto error = cudaHostRegister(buffers[i].data(), buffers[i].size(), 0);
      if (error == cudaErrorMemoryAllocation || error == cudaErrorNotSupported ||
          error == cudaErrorHostMemoryAlreadyRegistered) {
        // Includes separate outputs sharing a host page or externally pinned data.
        // Undo only our own registrations; ordinary copies remain correct.
        release();
        cudaGetLastError();
        return 0;
      }

      checked(error);
      pointers[i] = buffers[i].data();
    }

    return total;
  }
};

template <class T> struct Buffer {
  Memory& memory;
  T* data = nullptr;
  std::size_t capacity = 0;

  explicit Buffer(Memory& m) : memory(m) {
  }

  ~Buffer() {
    if (data)
      cudaFree(data);
  }

  void reserve(std::size_t count) {
    if (count <= capacity)
      return;

    if (count > std::numeric_limits<std::size_t>::max() / sizeof(T))
      throw std::length_error("device buffer byte count overflow");

    T* fresh = nullptr;
    checked(cudaMalloc(&fresh, count * sizeof(T)));
    memory.current += count * sizeof(T);
    memory.peak = std::max(memory.peak, memory.current);

    if (data) {
      const auto error = cudaFree(data);
      if (error != cudaSuccess) {
        cudaFree(fresh);
        memory.current -= count * sizeof(T);
        checked(error);
      }
    }

    memory.current -= capacity * sizeof(T);
    data = fresh;
    capacity = count;
  }
};

// Capture straight-line stages into an existing graph; explicitly inserted
// conditional nodes own the loops. CUB therefore keeps its normal host API.
struct GraphChain {
  cudaGraph_t graph;
  cudaStream_t stream;
  std::vector<cudaGraphNode_t> tails;

  void record(cudaEvent_t event) {
    cudaGraphNode_t node;
    checked(cudaGraphAddEventRecordNode(&node, graph, tails.data(), tails.size(), event));
    tails = {node};
  }

  template <class F> void capture(F&& f) {
    checked(cudaStreamBeginCaptureToGraph(
        stream, graph, tails.data(), nullptr, tails.size(), cudaStreamCaptureModeThreadLocal));
    try {
      f();
      checked(cudaGetLastError());

      const cudaGraphNode_t* dependencies = nullptr;
      std::size_t count = 0;
      cudaStreamCaptureStatus status;
      checked(cudaStreamGetCaptureInfo(
          stream, &status, nullptr, nullptr, &dependencies, nullptr, &count));
      tails.assign(dependencies, dependencies + count);

      cudaGraph_t captured = nullptr;
      checked(cudaStreamEndCapture(stream, &captured));
    } catch (...) {
      cudaGraph_t ignored = nullptr;
      cudaStreamEndCapture(stream, &ignored);
      throw;
    }
  }

  GraphChain conditional(cudaGraphConditionalHandle handle, cudaGraphConditionalNodeType type) {
    cudaGraphNodeParams params{};
    params.type = cudaGraphNodeTypeConditional;
    params.conditional.handle = handle;
    params.conditional.type = type;
    params.conditional.size = 1;

    cudaGraphNode_t node;
    checked(cudaGraphAddNode(&node, graph, tails.data(), nullptr, tails.size(), &params));
    tails = {node};

    return {params.conditional.phGraph_out[0], stream, {}};
  }
};

struct Configuration {
  // GPU initialization counts arrows and validates the caller's allocation bound.
  int rows, equations, arrow_capacity, bases;
  Checker checker;
  bool instrument;
  bool operator==(const Configuration&) const = default;
};

}  // namespace

struct Workspace::Impl {
  Memory memory;
  Buffer<TInfo> arena{memory};
  Buffer<TypeId> parents{memory}, arrows{memory};
  Buffer<Equation> requests{memory};
  // Source input and immutable outputs; requests are overwritten during solving.
  Buffer<DphmSourceNode> source_nodes{memory};
  Buffer<std::uint64_t> source_weights_buffer{memory}, source_prefix{memory};
  Buffer<TypeId> source_types{memory};
  Buffer<DphmBinderTypes> source_binders{memory};
  Buffer<int> source_invalid{memory};
  Buffer<unsigned char> source_scratch{memory};
  Buffer<DphmArrow> result_arrows{memory};
  Buffer<DphmNodeTyping> result_source{memory};
  Buffer<State> state{memory};
  Buffer<TypeId> signatures{memory};
  Buffer<unsigned char> finished0{memory}, finished1{memory};
  cudaStream_t stream = nullptr;
  std::array<cudaEvent_t, 4> transfers{};
  std::array<cudaEvent_t, 2> inference{};
  std::array<cudaEvent_t, 2> lowering{};
  cudaEvent_t exported = nullptr;
  cudaGraph_t graph = nullptr;
  cudaGraphExec_t executable = nullptr;
  Configuration configuration{};
  int device = 0, grid_limit = 1;
  Metrics metrics;

  Impl() {
    checked(cudaGetDevice(&device));
    int multiprocessors = 0;
    checked(cudaDeviceGetAttribute(&multiprocessors, cudaDevAttrMultiProcessorCount, device));
    grid_limit = 8 * multiprocessors;

    checked(cudaStreamCreateWithFlags(&stream, cudaStreamNonBlocking));

    try {
      for (auto& event : transfers)
        checked(cudaEventCreate(&event));
      for (auto& event : inference)
        checked(cudaEventCreate(&event));
      for (auto& event : lowering)
        checked(cudaEventCreate(&event));
      checked(cudaEventCreate(&exported));
    } catch (...) {
      for (auto event : transfers)
        if (event)
          cudaEventDestroy(event);
      for (auto event : inference)
        if (event)
          cudaEventDestroy(event);
      for (auto event : lowering)
        if (event)
          cudaEventDestroy(event);
      if (exported)
        cudaEventDestroy(exported);

      cudaStreamDestroy(stream);
      throw;
    }
  }

  ~Impl() {
    cudaStreamSynchronize(stream);
    destroy_graph();

    for (auto event : transfers)
      cudaEventDestroy(event);
    for (auto event : inference)
      cudaEventDestroy(event);
    for (auto event : lowering)
      cudaEventDestroy(event);
    cudaEventDestroy(exported);
    cudaStreamDestroy(stream);
  }

  void destroy_graph() {
    if (executable)
      cudaGraphExecDestroy(executable);
    if (graph)
      cudaGraphDestroy(graph);

    executable = nullptr;
    graph = nullptr;
  }

  unsigned grid(int count) const {
    return std::min<unsigned>(blocks(count), grid_limit);
  }

  cudaGraphConditionalHandle handle() {
    cudaGraphConditionalHandle value;
    checked(cudaGraphConditionalHandleCreate(&value, graph, 0, cudaGraphCondAssignDefault));
    return value;
  }

  void step(GraphChain& chain, Step operation, cudaGraphConditionalHandle condition = 0) {
    chain.capture([&] {
      control<<<1, 1, 0, stream>>>(state.data, operation, condition);
    });
  }

  template <class F> void phase(GraphChain& chain, Phase name, F&& f) {
    chain.capture([&] {
      if (configuration.instrument)
        timestamp<<<1, 1, 0, stream>>>(state.data, name, true);

      f();

      if (configuration.instrument)
        timestamp<<<1, 1, 0, stream>>>(state.data, name, false);
    });
  }

  bool fused_down() const {
    return configuration.checker == Checker::a;
  }

  void close(GraphChain& chain, bool down_batch = false) {
    phase(chain, merge_phase, [&] {
      merge<<<grid(static_cast<int>(requests.capacity)), threads, 0, stream>>>(
          parents.data, requests.data, state.data, configuration.bases, down_batch);
    });
    phase(chain, flatten_phase, [&] {
      flatten<<<grid(configuration.rows), threads, 0, stream>>>(
          parents.data, configuration.rows, state.data);
    });
    if (!fused_down())
      phase(chain, validation_phase, [&] {
        validate_classes<<<grid(configuration.rows), threads, 0, stream>>>(
            arena.data, parents.data, configuration.rows, state.data);
      });
  }

  void reserve(const Configuration& c) {
    const auto start = Clock::now();

    arena.reserve(c.rows);
    parents.reserve(c.rows);
    state.reserve(1);
    arrows.reserve(std::max(1, c.arrow_capacity));
    requests.reserve(std::max({1, c.equations, frontier_capacity(c)}));
    finished0.reserve(c.rows);
    finished1.reserve(c.rows);
    if (c.checker == Checker::b && c.arrow_capacity)
      signatures.reserve(signature_capacity(c));

    metrics.allocation_ms = milliseconds(start);
  }

  static int signature_capacity(const Configuration& c) {
    // At most arrow_capacity distinct keys, hence at most 50% occupancy.
    // clang-format off
    return static_cast<int>(
        cuco::make_valid_extent<SignatureProbing, SignatureStorage>(
            std::size_t{2} * c.arrow_capacity).value());
    // clang-format on
  }

  static int down_capacity(const Configuration& c) {
    return 2 * c.arrow_capacity;
  }

  static int frontier_capacity(const Configuration& c) {
    return 3 * down_capacity(c);
  }

  void build(const Configuration& c) {
    destroy_graph();
    reserve(c);
    configuration = c;

    const auto start = Clock::now();
    checked(cudaGraphCreate(&graph, 0));
    GraphChain root{graph, stream, {}};
    root.capture([&] {
      reset<<<1, 1, 0, stream>>>(
          state.data, c.equations, c.rows, static_cast<int>(requests.capacity));
    });
    phase(root, initialize_phase, [&] {
      initialize<<<grid(std::max(c.rows, c.equations)), threads, 0, stream>>>(arena.data,
          parents.data,
          arrows.data,
          c.rows,
          c.bases,
          c.arrow_capacity,
          requests.data,
          c.equations,
          state.data);
    });

    root.record(inference[0]);
    const auto valid = handle();
    step(root, Step::valid, valid);
    auto solve = root.conditional(valid, cudaGraphCondTypeIf);
    {
      close(solve);

      const auto macro = handle();
      step(solve, Step::valid, macro);
      auto loop = solve.conditional(macro, cudaGraphCondTypeWhile);
      step(loop, Step::macro_begin);
      phase(loop, down_phase, [&] {
        if (fused_down()) {
          // A's terminal Down also checks the last merge before accepting.
          const int count = c.bases + c.arrow_capacity;
          validate_down<<<grid(count), threads, 0, stream>>>(
              arena.data, parents.data, arrows.data, c.rows, c.bases, requests.data, state.data);
          return;
        }
        down<<<grid(c.arrow_capacity), threads, 0, stream>>>(
            arena.data, parents.data, arrows.data, requests.data, state.data);
      });

      if (fused_down()) {
        const auto publish = handle();
        phase(loop, u1_phase, [&] {
          expand<true><<<grid(down_capacity(c)), threads, 0, stream>>>(
              arena.data, parents.data, requests.data, state.data, macro, publish);
        });

        auto publication = loop.conditional(publish, cudaGraphCondTypeIf);
        close(publication, true);
      } else {
        const auto have_down = handle();
        step(loop, Step::down_ready, have_down);
        auto down_body = loop.conditional(have_down, cudaGraphCondTypeIf);
        phase(down_body, u1_phase, [&] {
          expand<><<<grid(down_capacity(c)), threads, 0, stream>>>(
              arena.data, parents.data, requests.data, state.data);
        });

        const auto down_valid = handle();
        step(down_body, Step::down_close, down_valid);
        auto down_close = down_body.conditional(down_valid, cudaGraphCondTypeIf);
        close(down_close);

        if (c.checker == Checker::b && c.arrow_capacity) {
          const auto up_valid = handle();
          step(loop, Step::up_begin, up_valid);
          auto up_body = loop.conditional(up_valid, cudaGraphCondTypeIf);
          phase(up_body, up_phase, [&] {
            const int capacity = signature_capacity(c);
            const FrozenSignatures keys{arena.data, parents.data};
            SignatureSet set{cuco::empty_key<TypeId>{0},
                keys,
                SignatureProbing{keys},
                {},
                SignatureStorage{static_cast<std::size_t>(capacity), signatures.data}};

            clear_signatures<<<grid(capacity), threads, 0, stream>>>(
                signatures.data, capacity, state.data);
            hash_up<<<grid(c.arrow_capacity), threads, 0, stream>>>(
                parents.data, arrows.data, set, requests.data, state.data);
          });

          const auto have_up = handle();
          step(up_body, Step::up_ready, have_up);
          auto up_close = up_body.conditional(have_up, cudaGraphCondTypeIf);
          close(up_close);
        }

        step(loop, Step::macro_end, macro);
      }
    }

    const auto occurs = handle();
    step(solve, Step::occurs_begin, occurs);
    auto occurs_body = solve.conditional(occurs, cudaGraphCondTypeIf);
    if (c.instrument)
      occurs_body.capture([&] {
        timestamp<<<1, 1, 0, stream>>>(state.data, occurs_phase, true);
      });
    occurs_body.capture([&] {
      seed_finished<<<grid(c.rows), threads, 0, stream>>>(
          arena.data, parents.data, c.rows, finished0.data, finished1.data, state.data);
    });

    const auto peeling = handle();
    step(occurs_body, Step::valid, peeling);
    auto waves = occurs_body.conditional(peeling, cudaGraphCondTypeWhile);
    step(waves, Step::peel_begin);
    waves.capture([&] {
      peel<<<grid(c.arrow_capacity), threads, 0, stream>>>(
          arena.data, parents.data, arrows.data, finished0.data, finished1.data, state.data);
    });

    step(waves, Step::peel_end, peeling);
    if (c.instrument)
      occurs_body.capture([&] {
        timestamp<<<1, 1, 0, stream>>>(state.data, occurs_phase, false);
      });
    // Root dependency covers the entire conditional solve, including rejection.
    root.record(inference[1]);
    checked(cudaGraphInstantiate(&executable, graph, 0));
    metrics.graph_build_ms = milliseconds(start);
  }

  Result result(Clock::time_point start, Status status) {
    metrics.native_ms = milliseconds(start);
    metrics.workspace_bytes = memory.current;
    metrics.peak_device_bytes = memory.peak;

    return {status, metrics};
  }

  // Deliver normalized source types, never the mutable request frontier.
  Status execute(const TypingOutput& output) {
    State finished;

    // Keep the status destination alive even if a later CUDA API call throws.
    try {
      const auto instrument = configuration.instrument;
      checked(cudaGraphLaunch(executable, stream));
      ++metrics.graph_launches;

      {
        const auto n = static_cast<int>(output.source.size());
        const auto f = static_cast<int>(output.arrows.size());
        source_export<<<grid(n), threads, 0, stream>>>(arena.data,
            parents.data,
            source_types.data,
            source_binders.data,
            n,
            configuration.rows - f,
            f,
            result_arrows.data,
            result_source.data);
        checked(cudaGetLastError());
        ++metrics.kernel_launches;
        checked(cudaEventRecord(exported, stream));
      }
      if (instrument)
        checked(cudaEventRecord(transfers[2], stream));
      if (!output.arrows.empty())
        checked(cudaMemcpyAsync(output.arrows.data(),
            result_arrows.data,
            output.arrows.size_bytes(),
            cudaMemcpyDeviceToHost,
            stream));
      checked(cudaMemcpyAsync(output.source.data(),
          result_source.data,
          output.source.size_bytes(),
          cudaMemcpyDeviceToHost,
          stream));
      checked(
          cudaMemcpyAsync(&finished, state.data, sizeof(finished), cudaMemcpyDeviceToHost, stream));
      if (instrument)
        checked(cudaEventRecord(transfers[3], stream));
      checked(cudaStreamSynchronize(stream));
      ++metrics.synchronizations;

      float elapsed = 0;
      checked(cudaEventElapsedTime(&elapsed, inference[0], inference[1]));
      metrics.inference_ms = elapsed;
      checked(cudaEventElapsedTime(&elapsed, lowering[0], exported));
      metrics.source_device_ms = elapsed;
      checked(cudaEventElapsedTime(&elapsed, inference[1], exported));
      metrics.export_ms = elapsed;

      metrics.kernel_launches += finished.kernels;
      metrics.primitive_calls += finished.primitives;
      metrics.macros = finished.macros;
      metrics.down_batches = finished.down_batches;
      metrics.up_batches = finished.up_batches;
      metrics.occurs_waves = finished.occurs_waves;
      metrics.generated_equations = finished.generated;

      if (instrument) {
        for (int i = 0; i < phase_count; ++i)
          metrics.phase_ms[phase_names[i]] = finished.nanoseconds[i] * 1e-6;
        metrics.phase_ms["bulk_close"] = metrics.phase_ms["merge"] + metrics.phase_ms["flatten"] +
                                         metrics.phase_ms["class_validation"];
        for (int i = 0; i < 2; ++i) {
          checked(cudaEventElapsedTime(&elapsed, transfers[2 * i], transfers[2 * i + 1]));
          metrics.phase_ms[i == 0 ? "upload" : "download"] = elapsed;
        }
      }
      if (finished.status == 4)
        throw std::runtime_error("CUDA solver invariant failure");

      return static_cast<Status>(finished.status);
    } catch (...) {
      cudaStreamSynchronize(stream);
      throw;
    }
  }
};

Workspace::Workspace() : impl_(std::make_unique<Impl>()) {
}

Workspace::~Workspace() = default;

Result Workspace::infer_ast(std::span<const DphmSourceNode> nodes,
    std::size_t variables,
    std::size_t arrows,
    Options options,
    const TypingOutput& output) {
  auto& s = *impl_;
  const auto start = Clock::now();
  s.metrics = {};
  s.memory.peak = s.memory.current;

  OutputRegistration registered;
  auto result = [&](Status status) {
    const auto unpin = Clock::now();
    registered.release();
    s.metrics.host_unregister_ms = milliseconds(unpin);
    return s.result(start, status);
  };

  // Bound every allocation/index before adding counts or narrowing to int.
  constexpr std::size_t limit = INT_MAX / 12;
  if (nodes.empty() || nodes.size() > limit || variables > limit || arrows > limit ||
      variables + arrows + 2 > limit)
    return result(Status::invalid_input);
  const auto rows = 3 + variables + arrows;
  const int n = static_cast<int>(nodes.size());
  if (output.arrows.size() != arrows || output.source.size() != nodes.size())
    return result(Status::invalid_input);

  int device = 0;
  checked(cudaGetDevice(&device));
  if (device != s.device)
    throw std::runtime_error("workspace used on a different CUDA device");
  checked(cudaGetLastError());

  int invalid = 0;  // Outlives the catch's completion wait for an asynchronous copy.
  try {
    if (options.register_output) {
      const auto pin = Clock::now();
      s.metrics.registered_output_bytes = registered.acquire(output);
      s.metrics.host_register_ms = milliseconds(pin);
    }

    const auto prepare = Clock::now();
    const Configuration c{static_cast<int>(rows),
        static_cast<int>(arrows),
        static_cast<int>(arrows),
        2,
        options.checker,
        options.instrument};
    s.metrics.host_prepare_ms = milliseconds(prepare);
    if (!s.executable || !(s.configuration == c))
      s.build(c);

    const auto allocation = Clock::now();
    s.source_nodes.reserve(nodes.size());
    s.source_weights_buffer.reserve(nodes.size() + 1);
    s.source_prefix.reserve(nodes.size() + 1);
    s.source_types.reserve(nodes.size());
    s.source_binders.reserve(nodes.size());
    s.source_invalid.reserve(1);
    s.result_arrows.reserve(arrows);
    s.result_source.reserve(nodes.size());

    std::size_t bytes = 0;
    checked(cub::DeviceScan::ExclusiveSum(
        nullptr, bytes, s.source_weights_buffer.data, s.source_prefix.data, n + 1, s.stream));
    s.source_scratch.reserve(bytes);
    s.metrics.allocation_ms += milliseconds(allocation);

    if (options.instrument)
      checked(cudaEventRecord(s.transfers[0], s.stream));
    checked(cudaMemcpyAsync(
        s.source_nodes.data, nodes.data(), nodes.size_bytes(), cudaMemcpyHostToDevice, s.stream));
    if (options.instrument)
      checked(cudaEventRecord(s.transfers[1], s.stream));
    checked(cudaEventRecord(s.lowering[0], s.stream));

    checked(cudaMemsetAsync(s.source_invalid.data, 0, sizeof(int), s.stream));
    source_weights<<<s.grid(n + 1), threads, 0, s.stream>>>(
        s.source_nodes.data, s.source_weights_buffer.data, n);
    checked(cub::DeviceScan::ExclusiveSum(s.source_scratch.data,
        bytes,
        s.source_weights_buffer.data,
        s.source_prefix.data,
        n + 1,
        s.stream));
    source_emit<<<s.grid(n), threads, 0, s.stream>>>(s.source_nodes.data,
        s.source_prefix.data,
        n,
        static_cast<int>(variables),
        static_cast<int>(arrows),
        s.arena.data,
        s.requests.data,
        s.source_types.data,
        s.source_binders.data,
        s.source_invalid.data);
    checked(cudaGetLastError());
    checked(cudaEventRecord(s.lowering[1], s.stream));
    s.metrics.kernel_launches = 2;
    s.metrics.primitive_calls = 1;

    checked(cudaMemcpyAsync(
        &invalid, s.source_invalid.data, sizeof(invalid), cudaMemcpyDeviceToHost, s.stream));
    checked(cudaStreamSynchronize(s.stream));
    ++s.metrics.synchronizations;

    float elapsed = 0;
    checked(cudaEventElapsedTime(&elapsed, s.lowering[0], s.lowering[1]));
    s.metrics.lowering_ms = elapsed;
    if (invalid)
      return result(Status::invalid_input);

    return result(s.execute(output));
  } catch (...) {
    cudaStreamSynchronize(s.stream);  // Caller-owned copy buffers must outlive queued work.
    throw;
  }
}

}  // namespace dphm::cuda_solver
