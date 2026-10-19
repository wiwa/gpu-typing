<!-- Generated/maintained with AI; see repository disclaimers.md. -->
# gpu-typing: data-parallel type inference on GPUs

DPHM means Data-Parallel Hindley–Milner. This repository implements its
monomorphic fragment: closed unary STLC with Bool/Int, without let-generalization.
The [algorithm guide](dphm/docs/algorithm.md) defines the phases and representation;
the [introductory blog post](https://winwang.blog/posts/dphm-1) gives a walkthrough.

We use Rust -> CUDA instead of simple C++/CUDA, so we can read less C++.

There are five implementations:

1. **[Pedagogical Python](dphm/dphm_simple.py):** standalone runnable pseudocode.
   Sequential loops model bulk merging and one-level Down propagation, followed
   by occurs checking. No U1 or Up; its public result is the root type.
2. **[Rust arena-eager](dphm/rust/src/sequential.rs):** conventional eager
   unification of local typing equations, using union-find and a final occurs check.
3. **[Rust Algorithm J](dphm/rust/src/j.rs):** this is the one to look at if you 
   want to check out what a typechecking algorithm should look like.
   It is syntax-directed inference with mutable types and binding-time occurs checks.
   The source maps J's typing rules to its iterative traversal.
4. **[Rust compact J-style](dphm/rust/src/j_compact.rs):** an optimized variant
   with packed type records, known-arrow reuse and a final occurs check.
5. **[CUDA DPHM](dphm/cuda_solver/solver.cu):** parallel source lowering and
   bulk-synchronous inference. Schedule A uses Down/U1; B additionally closes
   upward arrow congruence. Both return shared types for the full source tree.

The Rust implementations share a whole-tree result interface and provide the
CUDA bindings. Supporting files contain the AST representation, build setup,
examples and correctness tests—not additional inference engines.

The Python sketch is effectively equivalent to `UNIFY` as laid out in the paper
"N-Axioms Parallel Unification" (2003) by Bellia and Occhiuto.

## Read and run

- [Build instructions and API guide](dphm/README.md)
- [Algorithm and representation](dphm/docs/algorithm.md)
- [Correctness and parallelism rationale](dphm/docs/rationale.md)
- [Scope and assumptions](dphm/docs/limitations.md)
- [AI involvement](disclaimers.md)

For the Python examples, with Python 3.10+ and no third-party dependencies:

```sh
python3 -B dphm/dphm_simple.py
```

The Python and Rust CPU implementations need no GPU. For the Rust tests, with
Rust 1.89 or newer:

```sh
cargo test --locked --release --manifest-path dphm/rust/Cargo.toml
```

The CUDA solver needs Linux and an sm_89 or sm_120 NVIDIA GPU; see its
[build instructions](dphm/README.md#cuda).

Project code is [MIT licensed](LICENSE); dependencies retain their own licenses.
