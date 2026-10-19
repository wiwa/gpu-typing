<!-- Generated/maintained with AI; see repository disclaimers.md. -->
# Scope and assumptions

## Language and results

The language is closed unary STLC with Bool/Int and finite principal types.
There is no let-polymorphism, subtyping, typeclass search, higher-rank inference,
module system or source parser. Binder references must be resolved and in scope.

The Python teaching implementation reconstructs the root type. Rust and CUDA
return a shared type DAG with every source node and binder's type reference in
one variable namespace. Expanding a shared type into a tree can require
exponential output.

CUDA A does not globally merge equal arrow shapes. Normalized references support
one-level queries, not constant-time structural equality between arbitrary types.
B additionally merges equal normalized arrow signatures. This canonicalization
does not change the principal source typings.

## Execution and resources

Array storage alone does not guarantee low parallel depth: CAS contention,
parent-chain walks and synchronization remain real costs. Published logarithmic
connectivity bounds are not bounds for this CAS implementation. End-only occurs
checking can diagnose infinite types late; successful finite type depth does not
bound rejection latency.

Fixed-arity working arrays and shared output have linear-size storage, but finite
ID/frontier limits and allocation can reject oversized inputs. Malformed input,
resource exhaustion and runtime failure are distinct from typing rejection.
For an input with multiple typing errors, the reported error kind depends on
traversal and scheduling; only acceptance/rejection is shared across engines.
The Rust CUDA workspace refuses reuse after a native runtime/API failure;
ordinary typing rejection does not invalidate it.

## Correctness boundary

The [rationale](rationale.md) gives source-level correctness arguments, not a
machine-checked refinement to CUDA instructions. Parallel execution relies on
correct count/scan/emission, exact CAS publication, rigid representative choice,
disjoint output ownership and ordered mutation/read phases.

Tests and CPU/GPU agreement provide bounded evidence, not a proof for every input
or device. CUDA/CUB/cuCollections, Rust/C++ toolchains, allocation and execution
remain platform dependencies. The project MIT license covers project code;
dependencies retain their upstream licenses.
