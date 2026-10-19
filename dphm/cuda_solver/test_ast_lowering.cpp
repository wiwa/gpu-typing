// Generated/maintained with AI; see repository disclaimers.md.
// Host-only tests of the actual emission code. Not CUDA scan/stream/race tests.
#include "c_api.h"
#include <cassert>
#include <cstdint>
#include <vector>

namespace {
using TypeId = std::uint32_t;
using TInfo = DphmType;
using Equation = DphmEquation;
int lane, width;

int index() {
  return lane;
}

int stride() {
  return width;
}

void atomicExch(int* p, int value) {
  *p = value;
}

#define __global__
#define __device__
#include "ast_lowering.cuh"
#undef __device__
#undef __global__

void check(const std::vector<DphmSourceNode>& nodes,
    int variables,
    int arrows,
    bool valid,
    bool application = false) {
  for (width = 1; width <= 17; ++width) {
    const int n = nodes.size();
    std::vector<std::uint64_t> weights(n + 1), prefix(n + 1);
    for (lane = 0; lane < width; ++lane)
      source_weights(nodes.data(), weights.data(), n);

    std::uint64_t sum = 0;
    for (int i = 0; i <= n; ++i) {
      prefix[i] = sum;
      sum += weights[i];
    }

    std::vector<TInfo> arena(3 + variables + arrows);
    std::vector<Equation> equations(arrows);
    std::vector<TypeId> types(n);
    std::vector<DphmBinderTypes> binders(n);
    int invalid = 0;
    for (lane = 0; lane < width; ++lane)
      source_emit(nodes.data(),
          prefix.data(),
          n,
          variables,
          arrows,
          arena.data(),
          equations.data(),
          types.data(),
          binders.data(),
          &invalid);

    assert((invalid == 0) == valid);
    if (!valid)
      continue;
    // Rows start zeroed; every written row past 0 has a nonzero result.
    for (std::size_t i = 1; i < arena.size(); ++i)
      assert(arena[i].ret != 0);
    assert(arena[1].arg1 == DPHM_BASE_ARG && arena[2].arg1 == DPHM_BASE_ARG);

    std::vector<TypeId> parents(arena.size());
    for (std::size_t i = 0; i < parents.size(); ++i)
      parents[i] = i;
    if (variables)
      parents[3] = 2;  // Export must resolve IDs, not just copy them.

    std::vector<DphmArrow> arrow_types(arrows);
    std::vector<DphmNodeTyping> source(n);
    for (lane = 0; lane < width; ++lane)
      source_export(arena.data(),
          parents.data(),
          types.data(),
          binders.data(),
          n,
          3 + variables,
          arrows,
          arrow_types.data(),
          source.data());

    for (int i = 0; i < n; ++i) {
      assert(source[i].type_id == parents[types[i]]);
      assert(source[i].binder_id == parents[binders[i].first]);
    }
    for (int i = 0; i < arrows; ++i) {
      assert(arrow_types[i].arg == parents[arena[3 + variables + i].arg1]);
      assert(arrow_types[i].ret == parents[arena[3 + variables + i].ret]);
    }

    if (application) {
      assert((types == std::vector<TypeId>{3, 4, 5, 2}));
      assert(binders[1].first == 5 && binders[1].second == 0);
      assert(arena[6].arg1 == 2 && arena[6].ret == 3);
      assert(arena[7].arg1 == 5 && arena[7].ret == 5);
      assert(equations[0].left == 4 && equations[0].right == 6);
      assert(equations[1].left == 4 && equations[1].right == 7);
    }
  }
}
}  // namespace

int main() {
  check({{3, 2, 0}}, 0, 0, true);  // No variables/arrows/equations.
  check({{1, 1, 0}, {0, 0, 0}}, 2, 1, true);
  check({{0, 1, 0}, {1, 0, 0}}, 2, 1, true);  // Reversed node order.
  check({{2, 1, 3}, {1, 2, 0}, {0, 1, 0}, {3, 2, 0}}, 3, 2, true, true);
  check({{1, 99, 0}}, 2, 1, false);            // Bad child: no out-of-bounds access.
  check({{0, 99, 0}}, 0, 0, false);            // Bad binder.
  check({{3, 9, 0}}, 0, 0, false);             // Bad base.
  check({{99, 0, 0}}, 0, 0, false);            // Bad tag.
  check({{1, 1, 0}, {0, 0, 0}}, 0, 0, false);  // Bad counts.
}
