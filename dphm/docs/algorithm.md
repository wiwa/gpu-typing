<!-- Generated/maintained with AI; see repository disclaimers.md. -->
# Unary DPHM, from syntax to shared types

## Local constraints

Simply typed lambda calculus (STLC) here uses unary, one-argument functions.
The input is a closed tree of `Var(binder)`, `Lam(body)`, `App(f,x)` and base
occurrences Bool/Int. Binder references are already resolved. Syntax checking
validates scope/tree structure but neither assigns types nor solves constraints.

Reserve type IDs for variables and arrows. Each lambda/application owns one
arrow, so an exclusive prefix sum determines every node's output locations.
Each node then emits its local rule, with no recursive inference:

```text
Lam(body): type(lambda) = Fn(type(parameter), type(body))
App(f,x):  type(f)      = Fn(type(x), type(application))
Var(b):    uses b's parameter type ID
```

Each type ID's `TInfo` record holds one level, not an expanded type: an arrow
stores its argument and result IDs in `arg1` and `ret`. `arg1=0` marks a
variable; `UINT32_MAX` marks a base. Bool/Int occupy IDs 1/2; variables precede
the arrow range.

## Shared A/B phases, with U1/CAS

Rigid information means a known base or arrow head; variables have no head yet.
CAS means atomic compare-and-swap, used to publish parent links.

1. Bulk-merge the initial equations. A component containing rigid information
   selects a rigid representative, so structural readers see its type head.
2. **Down:** check rigid compatibility and compare each nonrepresentative arrow
   with its component's arrow representative, emitting argument/result equalities.
3. **U1:** on the frozen representatives, expand Down requests by one additional
   arrow level while retaining the parent requests. This is not recursive eager
   unification hidden inside one macro round.
4. Publish the requests through CAS linking, flatten parents, and repeat until
   no further requests remain. A final bottom-up peeling check rejects cycles.

Merge owns request edges; Down owns constructor records; U1 owns pending pairs;
flattening/peeling own array entries. These are grid-stride CUDA kernels, ordered
by a device-controlled graph. CAS retries and dependent parent walks remain real
costs; no worst-case O(log N) claim for this CAS implementation is implied.
Source lowering also runs on the GPU: count, scan and one local emission per
node. It is not a CPU type compiler feeding a pre-solved problem to the device.

**B adds Up after Down publication.** It clears a signature table and hashes each
arrow by its normalized argument/result pair. Threads finding equal signatures
emit parent-arrow equalities. A second CAS/flatten publication applies those
requests before the next macro round. Stop only when neither Down nor Up adds
requests; final cycle checking and result export are shared with A.

A is the default and more optimized path. B currently performs separate class
validation after each publication, while A fuses it into Down. B can also take
more rounds and perform more full-array flattening passes. This is a real
implementation difference, not an inherent cost attributed solely to Up.

The three CPU implementations use eager structural unification, not these bulk
rounds. Arena-eager streams local equations; direct J follows the syntax with
mutable types; compact J reuses known arrows and checks cycles at the end.
The standalone Python version teaches one-level Down without U1 or Up.

## Whole-tree result

`unary::Typing` contains normalized arrow-child pairs and a `[node,binder]`
type-reference pair per source node, plus the root and first-arrow ID. Zero
means no binder at that node. All references share one variable namespace.
One-level queries are O(1), with no GPU follow-up or host inference required.

Types remain a shared DAG: expanding a depth-D type can require exponential
output. The API keeps the shared graph instead. Since A omits Up, structurally
equal arrows need not have the same representative; that does not make their
materialized types unequal or their source typings incomplete.
At B's fixed point, equal normalized arrow signatures also have equal class
representatives. The conventional eager CPU implementations need not enforce this
additional internal canonicalization to infer the same principal source types.

## Reading the implementation

- [`rust/src/unary.rs`](../rust/src/unary.rs): checked AST, local lowering and
  whole-tree result contract.
- [`rust/src/sequential.rs`](../rust/src/sequential.rs): independent conventional
  eager inference.
- [`cuda_solver/ast_lowering.cuh`](../cuda_solver/ast_lowering.cuh): parallel
  emission/export, also host-testable.
- [`cuda_solver/solver.cu`](../cuda_solver/solver.cu): A/B phases and graph
  orchestration.
- [`rust/tests/unary_typing.rs`](../rust/tests/unary_typing.rs): CPU whole-tree
  checks; [`unary_differential.rs`](../rust/tests/unary_differential.rs) adds CUDA
  A/B checks and rejection/reuse coverage.
- [`rust/tests/j.rs`](../rust/tests/j.rs) and
  [`pipeline.rs`](../rust/tests/pipeline.rs): CPU agreement, known types and
  source maps.
- [`cuda_solver/test_ast_lowering.cpp`](../cuda_solver/test_ast_lowering.cpp):
  host-only source emission/export checks.
- [`cuda_solver/test_frontier.cu`](../cuda_solver/test_frontier.cu): GPU
  warp-append capacity/overflow checks.
- [`rust/src/j.rs`](../rust/src/j.rs) and
  [`j_compact.rs`](../rust/src/j_compact.rs): direct J and its optimized variant.
- [`dphm_simple.py`](../dphm_simple.py): standalone Down-only teaching
  implementation.
