// Generated/maintained with AI; see repository disclaimers.md.
#pragma once
#include "c_api.h"

#include <cstdint>
#include <map>
#include <memory>
#include <span>
#include <string>

namespace dphm::cuda_solver {

using TypeId = std::uint32_t;
using TInfo = DphmType;
using Equation = DphmEquation;
static_assert(sizeof(TInfo) == 8 && sizeof(Equation) == 8);
static_assert(alignof(TInfo) == 4 && offsetof(TInfo, ret) == 4);
static_assert(sizeof(DphmSourceNode) == 12 && sizeof(DphmBinderTypes) == 8);
static_assert(alignof(DphmSourceNode) == 4 && offsetof(DphmSourceNode, b) == 8);
static_assert(alignof(DphmBinderTypes) == 4 && offsetof(DphmBinderTypes, second) == 4);
static_assert(sizeof(DphmArrow) == 8 && sizeof(DphmNodeTyping) == 8);
static_assert(alignof(DphmArrow) == 4 && offsetof(DphmArrow, ret) == 4);
static_assert(alignof(DphmNodeTyping) == 4 && offsetof(DphmNodeTyping, binder_id) == 4);
static_assert(sizeof(DphmMetrics) == 232 && alignof(DphmMetrics) == 8);
static_assert(offsetof(DphmMetrics, peak_device_bytes) == 16);
static_assert(offsetof(DphmMetrics, macros) == 56 && offsetof(DphmMetrics, upload_ms) == 88);
static_assert(offsetof(DphmMetrics, generated_equations) == 152);
static_assert(offsetof(DphmMetrics, graph_launches) == 200);
static_assert(offsetof(DphmMetrics, registered_output_bytes) == 224);

enum class Checker {
  a,
  b
};

enum class Status {
  success,
  invalid_input,
  rigid_conflict,
  infinite_type
};

struct Options {
  Checker checker = Checker::a;
  bool instrument = true;       // Device phase timestamps; never introduces host waits.
  bool register_output = true;  // Page-lock the returned buffers during delivery.
};

struct Metrics {
  double native_ms = 0, allocation_ms = 0;
  double host_prepare_ms = 0, graph_build_ms = 0;
  double host_register_ms = 0, host_unregister_ms = 0;
  std::uint64_t registered_output_bytes = 0;
  // Device solving after initialization, including occurs; -1 if not measured.
  double inference_ms = -1;
  double lowering_ms = -1;       // Unary source scan/emission, before solver initialization.
  double source_device_ms = -1;  // Source lowering through whole-tree export completion.
  double export_ms = -1;         // Inference end through whole-tree export completion.
  std::uint64_t graph_launches = 0;
  // Owned device buffers only; excludes CUDA context/graph allocations.
  std::uint64_t peak_device_bytes = 0, workspace_bytes = 0;
  // Executed application kernels (not CUB internals); explicit final waits.
  std::uint64_t kernel_launches = 0, primitive_calls = 0, synchronizations = 0;
  std::uint64_t macros = 0, down_batches = 0, up_batches = 0, occurs_waves = 0;
  std::uint64_t generated_equations = 0;  // Selected Down/U1/Up requests submitted.
  std::map<std::string, double> phase_ms;
};

struct Result {
  Status status = Status::success;
  Metrics metrics;
};

struct TypingOutput {
  std::span<DphmArrow> arrows;
  std::span<DphmNodeTyping> source;
};

// Owns one stream and reusable linear-capacity buffers. Not concurrently usable.
// Inputs must stay immutable until inference returns; output must not overlap them.
class Workspace {
public:
  Workspace();
  ~Workspace();
  Workspace(const Workspace&) = delete;
  Workspace& operator=(const Workspace&) = delete;
  Result infer_ast(std::span<const DphmSourceNode> nodes,
      std::size_t variables,
      std::size_t arrows,
      Options options,
      const TypingOutput& output);

private:
  struct Impl;
  std::unique_ptr<Impl> impl_;
};

}  // namespace dphm::cuda_solver
