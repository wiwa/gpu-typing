<!-- Generated/maintained with AI; see repository disclaimers.md. -->
# Why it works, and why parallel execution can help

The starting point is ordinary unary STLC, not a new typing rule. Each source
node imposes a local typing requirement. The change is **how the equations are
solved**: advance a broad structural frontier instead of finishing one recursive
unification before moving to the next. [Algorithm](algorithm.md) describes the
records and kernels; this note explains the reasoning behind them.

## Correctness argument

(Proof by authority for Schedule A):  
2003 paper by Marco Bellia and Eugenia M. Occhiuto, their `UNIFY` algorithm:  
[N-axioms Parallel Unification](https://journals.sagepub.com/doi/abs/10.3233/FUN-2003-55203)


An arena ID denotes a type. A union-find component records IDs required to
denote the same type. Its representative carries a known constructor when the
component has one; otherwise its constructor is not yet known.

1. **The input equations are exactly the local typing requirements.** A lambda
   equates its type with `Fn(parameter,body)`; an application equates its
   function's type with `Fn(argument,result)`; variable occurrences share their
   binder's parameter ID. Assigning finite types satisfying these equations
   is equivalent to typing the closed source tree, by induction over its nodes.
2. **Bulk merging adds exactly the requested equalities.** IDs have an immutable
   total priority, rigid before flexible. A successful root-link CAS points to
   a preferred ID, preventing parent cycles. A failed CAS returns the installed
   parent; the thread carries the residual equality forward instead of dropping
   the request. Each continuing iteration improves its worse endpoint in the
   finite priority order, so requests terminate. Shortcuts connect nodes already
   in the same component.
   Thus installed links follow from requests, and completed requests hold in
   the resulting components.
   Overlapping requests are allowed; they need not be independent.
3. **Down loses no structural obligation.** Within an arrow component, comparing
   every arrow to one representative equates all corresponding arguments and
   results by transitivity. A pairwise comparison of every arrow is unnecessary.
   Different base types, or base versus arrow, cannot denote the same finite type
   and are rejected.
4. **U1 is sound lookahead, not a different solution.** An equality between two
   arrows already implies their corresponding child equalities. U1 adds those
   immediate consequences against the frozen state and retains the original
   requests. It cannot silently replace an unsatisfied parent equality with
   merely equal children. Delayed constructor conflicts are caught by the next
   mandatory Down/validation pass before acceptance.
5. **Saturation terminates even on bad inputs.** A continuing Down batch has an
   equality not already satisfied. Publication merges at least two classes.
   The arena is fixed, so there are at most linearly many productive publications
   in its size. No infinite recursive type expansion is performed.
6. **The final graph determines the finite principal answer.** Once Down is
   stable, all arrows in a component agree on their child components. A cycle
   would require finite type height to decrease back to itself, so is rejected.
   Semantically, interpret it bottom-up, assigning one fresh type variable per
   unconstrained component. This satisfies every equation. Any other finite
   solution comes from substituting for those variables: one joint principal
   typing for every node and binder, not separate unrelated answers per node.

An Up pass is not required for that result. Two unrelated arrow IDs whose
children become equal can remain separate while both denote the same type.
A omits this extra canonicalization, not a required typing constraint. B performs
it explicitly: equal child components imply equal arrow types, so each Up merge
preserves the finite solutions. Productive Up publication also reduces the
number of classes. At a stable Down/Up fixed point, the same principal readback
argument applies, with equal normalized arrow signatures additionally merged.

After rigid validation and Down saturation, a final-only cycle check suffices;
it need not run inside each macro-round. This does **not** promise early diagnosis
of infinite types. Kernel boundaries separate parent mutation from structural
reads, and array emissions own disjoint output ranges. This phase discipline is
the source argument for parallel execution, not a machine-checked CUDA
race-freedom proof.

## Why a GPU is plausible

There are linearly many initial records/equations in source size. Count/scan/
emission lowers the source without recursively inferring it. Down, bounded U1
expansion, parent normalization, cycle peeling and result export operate on
arrays with explicit per-item ownership. Many unrelated obligations can advance
in the same launch. One device-controlled graph runs the inference loop without
host decisions per round. Each call still waits on the host to validate lowering
before that graph, then waits again for final output delivery.

For a successful input let D be the maximum principal arrow height of any arena
ID, not just the root. The reference one-level Down schedule closes in O(D+1)
structural passes: a still-new child equality descends from parents newly equated
by the preceding pass, decreasing height in one fixed finite interpretation.
U1 includes that progress and may advance further. Final cycle peeling also
finishes in O(D+1) waves on such inputs.

This paragraph describes A's structural schedule. Do not use its pass count as
B's pass count: B must also propagate upward canonicalization to stability.

This is a bound on structural progress, **not the latency of a complete CUDA
round**. CAS merging and parent walks depend on equality topology, ID layout
and contention. Substituting a logarithmic-depth bulk-connectivity primitive
is a separate theoretical argument; that guarantee is not assigned to the CAS
backend here.

Each arrow emits at most two immediate Down pairs; U1 adds at most two children
per pair. Reusing these linear-size frontiers avoids an all-recursive-pairs
cache. Together with the fixed arena and shared output, this keeps storage
linear in source size for this fixed-arity language.

The favorable regime has substantial frontier width, modest type depth and
affordable connectivity. Small or narrow inputs can lose to launch/transfer
costs; deep or contended inputs can spend most of their time waiting on dependent
walks. We do not infer bandwidth saturation from array storage alone.

## Evidence boundary

These are source-level arguments, not a machine-checked refinement of the
compiled CUDA program. Correctness tests exercise the implementations but do
not replace the mathematical argument or validate every platform assumption.
See [scope and assumptions](limitations.md) for that boundary.
