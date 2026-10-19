// Generated/maintained with AI; see repository disclaimers.md.
// Unary source lowering: a scan assigns disjoint IDs; each node emits local rules.
// Included inside the solver's anonymous namespace; no inference on the host.
__global__ void source_weights(const DphmSourceNode* nodes, std::uint64_t* weights, int n) {
  for (int i = index(); i <= n; i += stride()) {
    const auto tag = i == n ? 0 : nodes[i].tag;
    // clang-format off
    weights[i] = tag == 1 ? (UINT64_C(1) << 32) + 2
               : tag == 2 ? (UINT64_C(1) << 32) + 1
                          : 0;
    // clang-format on
  }
}

// Each lane owns one arrow/source row. Read only final flat parents; no graph
// traversal or host reconstruction is needed to query the returned type DAG.
__global__ void source_export(const TInfo* arena,
    const TypeId* parents,
    const TypeId* node_types,
    const DphmBinderTypes* binders,
    int n,
    int first_arrow,
    int arrow_count,
    DphmArrow* arrows,
    DphmNodeTyping* source) {
  for (int i = index(); i < n; i += stride()) {
    source[i] = {parents[node_types[i]], parents[binders[i].first]};

    if (i < arrow_count) {
      const auto t = arena[first_arrow + i];
      arrows[i] = {parents[t.arg1], parents[t.ret]};
    }
  }
}

__device__ TypeId source_type(
    const DphmSourceNode* nodes, const std::uint64_t* prefix, int n, TypeId i, int* invalid) {
  if (i >= static_cast<TypeId>(n)) {
    atomicExch(invalid, 1);
    return 0;
  }

  const auto node = nodes[i];
  if (node.tag == 0) {
    if (node.a >= static_cast<TypeId>(n) || nodes[node.a].tag != 1 || node.b != 0) {
      atomicExch(invalid, 1);
      return 0;
    }
    return 4 + static_cast<TypeId>(prefix[node.a]);
  }

  if (node.tag == 3 && (node.a == 1 || node.a == 2) && node.b == 0)
    return node.a;
  if (node.tag == 1 || node.tag == 2)
    return 3 + static_cast<TypeId>(prefix[i]);

  atomicExch(invalid, 1);
  return 0;
}

__global__ void source_emit(const DphmSourceNode* nodes,
    const std::uint64_t* prefix,
    int n,
    int variables,
    int arrows,
    TInfo* arena,
    Equation* equations,
    TypeId* node_types,
    DphmBinderTypes* binder_types,
    int* invalid) {
  // The sentinel prefix is the total. A count mismatch must never index outputs.
  if (prefix[n] != (static_cast<std::uint64_t>(arrows) << 32) + variables) {
    if (index() == 0)
      atomicExch(invalid, 1);
    return;
  }

  if (index() == 0) {
    arena[0] = {0, 0};
    arena[1] = {DPHM_BASE_ARG, 1};
    arena[2] = {DPHM_BASE_ARG, 2};
  }

  for (int i = index(); i < n; i += stride()) {
    const auto node = nodes[i];
    // Compute child IDs from immutable syntax/prefix, never from another lane's output.
    const auto own = source_type(nodes, prefix, n, i, invalid);
    node_types[i] = own;
    binder_types[i] = {0, 0};

    if (node.tag != 1 && node.tag != 2)
      continue;
    if (node.tag == 1 && node.b != 0)
      atomicExch(invalid, 1);

    const auto equation = static_cast<TypeId>(prefix[i] >> 32);
    const auto tid = 3 + variables + equation;
    const auto a = source_type(nodes, prefix, n, node.a, invalid);
    const auto b = node.tag == 2 ? source_type(nodes, prefix, n, node.b, invalid) : own + 1;

    arena[own] = {0, own};
    if (node.tag == 1)
      arena[b] = {0, b};
    arena[tid] = {b, node.tag == 1 ? a : own};
    equations[equation] = {node.tag == 1 ? own : a, tid};
    if (node.tag == 1)
      binder_types[i].first = b;
  }
}
