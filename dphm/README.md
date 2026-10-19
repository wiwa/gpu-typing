<!-- Generated/maintained with AI; see repository disclaimers.md. -->
# DPHM

This directory contains the five implementations listed in the
[repository guide](../README.md), plus shared representations, bindings and
correctness tests. All infer closed unary STLC with Bool/Int; there is no
let-polymorphism, source parser or complete compiler.

## Pedagogical Python

From this directory, with Python 3.10+:

```sh
python3 -B dphm_simple.py
```

[dphm_simple.py](dphm_simple.py) uses the standard library only. It demonstrates
local constraint generation, bulk merging, one Down level per round and final
occurs checking. It omits U1 and Up. Its public `infer()` reconstructs the root
type; Python's sequential loops model bulk operations, not parallel execution.

## Rust CPU implementations

From the repository root, with Rust 1.89 or newer:

```sh
cargo test --locked --release
cargo run --locked --release --example unary
```

Prepare a checked input with `unary::ResolvedAst::new(&ast)`, then choose:

1. **[Arena-eager](rust/src/sequential.rs)**: `unary::infer_types(&source)`
   (in [unary.rs](rust/src/unary.rs)) streams local equations into eager
   union-find unification. This is the default CPU path used by the example.
2. **[Algorithm J](rust/src/j.rs)**: `j::infer_types(&source)`. Variables look up
   their binder's mutable type; lambdas introduce fresh parameter types;
   applications immediately unify the function with `argument -> fresh_result`.
   Type links carry substitutions, with binding-time occurs checks. An explicit
   stack replaces recursion; output conversion is a separate section.
3. **[Compact J-style](rust/src/j_compact.rs)**: `j_compact::infer_types(&source)`
   uses packed records, known-arrow reuse and one final occurs check.

None uses DPHM macro rounds or global upward congruence closure. All return
`unary::Typing`: shared arrow children and node/binder type references in one
type-variable namespace. The example applies `\x. x` to an Int literal and queries
its whole-tree result without expanding types into trees.

## CUDA

Supported CUDA targets are **sm_89 and sm_120 only**. The build requires Linux,
a matching NVIDIA GPU and Pixi 0.81 or newer. The lockfile pins Rust, CUDA 13.3,
C++23-capable GCC and build tools. The installed driver must support that toolkit;
Pixi does not install it. CMake fetches hash-pinned cuCollections headers during
configuration, so a first build needs network access. From this directory:

```sh
cd cuda_solver
pixi run rust-test
pixi run native-test
pixi run unary-example
pixi run unary-example-b
```

These commands build the native C ABI library and matching Rust bindings, then
exercise schedules A and B. A uses Down/U1; B adds global upward congruence
closure and is less optimized. Rebuild native code and bindings together after
API changes. No Python binding or host inference callback is used.

For reading the implementation, [solver.cu](cuda_solver/solver.cu) contains the
kernels and their A/B schedule. [runtime_support.hpp](cuda_solver/runtime_support.hpp)
holds private buffer, output-registration and graph-wiring helpers.

A fresh build includes both targets as native cubins, without PTX. To build only
one of them, from `cuda_solver/`:

```sh
pixi run configure -DCMAKE_CUDA_ARCHITECTURES=89-real
pixi run build
```

The architecture selection is cached. Only `89-real`, `120-real`, or both are
accepted; any other value, including `native` or `all`, is rejected at configure
time.
`pixi run arch-check` compile-checks 89 and 120 without GPU execution;
`pixi run arch-check 89` checks only 89.
It uses GNU `timeout` to bound each build. Compile checks do not establish runtime
correctness on another GPU or operating system.

`native-test` runs source emission/export checks on the host and warp-frontier
capacity checks on the GPU, with assertions enabled. The frontier test includes
exact-fit, offset and overflowing concurrent reservations; it is not a general
race-freedom test.

CUDA A/B correctness tests last passed on Linux, CUDA 13.3, compute capability
12.0. Target 8.9 is compile-checked, not runtime-validated here.

## Development checks

From the repository root:

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

C++/CUDA sources follow the root `.clang-format`.

Read the [algorithm](docs/algorithm.md), [rationale](docs/rationale.md),
[limitations](docs/limitations.md) and [AI disclosure](../disclaimers.md).
Project code is [MIT licensed](../LICENSE); dependencies retain their own licenses.
