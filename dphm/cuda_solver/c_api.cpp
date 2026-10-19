// Generated/maintained with AI; see repository disclaimers.md.
#include "c_api.h"
#include "solver.hpp"
#include <cstdio>
#include <climits>
#include <exception>
#include <stdexcept>

using namespace dphm::cuda_solver;

struct DphmWorkspace {
  Workspace solver;
  double inference_ms = -1;
};

namespace {
template <class F> int boundary(char* error, size_t capacity, F&& action) noexcept {
  if (error && capacity)
    error[0] = '\0';

  try {
    return action();
  } catch (const std::exception& e) {
    if (error && capacity)
      std::snprintf(error, capacity, "%s", e.what());
  } catch (...) {
    if (error && capacity)
      std::snprintf(error, capacity, "unknown native exception");
  }

  return -1;
}

int publish(DphmWorkspace* workspace, const Result& result, DphmMetrics* metrics) {
  const auto& m = result.metrics;
  workspace->inference_ms = m.inference_ms;

  auto phase = [&](const char* name) {
    const auto it = m.phase_ms.find(name);
    return it == m.phase_ms.end() ? 0.0 : it->second;
  };

  *metrics = {m.native_ms,
      m.allocation_ms,
      m.peak_device_bytes,
      m.workspace_bytes,
      m.kernel_launches,
      m.primitive_calls,
      m.synchronizations,
      m.macros,
      m.down_batches,
      m.up_batches,
      m.occurs_waves,
      phase("upload"),
      phase("initialize"),
      phase("down"),
      phase("u1"),
      phase("up"),
      phase("bulk_close"),
      phase("occurs"),
      phase("download"),
      m.generated_equations,
      phase("merge"),
      phase("flatten"),
      phase("class_validation"),
      m.host_prepare_ms,
      m.graph_build_ms,
      m.graph_launches,
      m.host_register_ms,
      m.host_unregister_ms,
      m.registered_output_bytes};

  switch (result.status) {
  case Status::success:
    return 0;
  case Status::invalid_input:
    return 1;
  case Status::rigid_conflict:
    return 2;
  case Status::infinite_type:
    return 3;
  }
  throw std::logic_error("unknown solver status");
}
}

int dphm_create(DphmWorkspace** out, char* error, size_t capacity) {
  return boundary(error, capacity, [&] {
    if (!out)
      throw std::invalid_argument("null workspace output");
    *out = nullptr;
    *out = new DphmWorkspace();
    return 0;
  });
}

void dphm_destroy(DphmWorkspace* workspace) {
  delete workspace;
}

double dphm_inference_ms(const DphmWorkspace* workspace) {
  return workspace ? workspace->inference_ms : -1;
}

int dphm_infer_unary_ast(DphmWorkspace* workspace,
    const DphmSourceNode* nodes,
    size_t count,
    size_t variables,
    size_t arrows,
    int checker,
    int instrument,
    int register_output,
    DphmArrow* arrow_types,
    DphmNodeTyping* source,
    DphmMetrics* metrics,
    double* lowering_ms,
    double* device_ms,
    double* export_ms,
    char* error,
    size_t capacity) {
  if (workspace)
    workspace->inference_ms = -1;
  if (lowering_ms)
    *lowering_ms = -1;
  if (device_ms)
    *device_ms = -1;
  if (export_ms)
    *export_ms = -1;

  return boundary(error, capacity, [&] {
    if (!workspace || !metrics || !lowering_ms || !device_ms || !export_ms || !nodes || !source ||
        (arrows && !arrow_types) || checker < 0 || checker > 1 || register_output < 0 ||
        register_output > 1)
      throw std::invalid_argument("invalid unary typing API argument");

    *metrics = {};
    const TypingOutput output{{arrow_types, arrows}, {source, count}};
    const auto result = workspace->solver.infer_ast({nodes, count},
        variables,
        arrows,
        {checker ? Checker::b : Checker::a, instrument != 0, register_output != 0},
        output);

    *lowering_ms = result.metrics.lowering_ms;
    *device_ms = result.metrics.source_device_ms;
    *export_ms = result.metrics.export_ms;
    return publish(workspace, result, metrics);
  });
}
