// Generated/maintained with AI; see repository disclaimers.md.
// Private host-side CUDA support; no typing rules or A/B phase decisions.
#pragma once
#include "solver.hpp"
#include <cuda_runtime.h>

#include <algorithm>
#include <array>
#include <cstddef>
#include <limits>
#include <span>
#include <stdexcept>
#include <string>
#include <vector>

namespace dphm::cuda_solver::detail {

inline void checked(cudaError_t error) {
  if (error != cudaSuccess)
    throw std::runtime_error(
        std::string(cudaGetErrorName(error)) + ": " + cudaGetErrorString(error));
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

}  // namespace dphm::cuda_solver::detail
