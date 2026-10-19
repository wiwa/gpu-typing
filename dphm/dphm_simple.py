# SPDX-License-Identifier: MIT
# Generated/maintained with AI; see repository disclaimers.md.
# Copyright (c) 2026 Win Wang

# Permission is hereby granted, free of charge, to any person obtaining a copy of
# this software and associated documentation files (the "Software"),
# to deal in the Software without restriction, including without limitation
# the rights to use, copy, modify, merge, publish, distribute, sublicense,
# and/or sell copies of the Software, and to permit persons to whom the Software
# is furnished to do so, subject to the following conditions:

# The above copyright notice and this permission notice shall be included in
# all copies or substantial portions of the Software.

# THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED,
# INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
# FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
# IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
# DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT,
# TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR
# THE USE OR OTHER DEALINGS IN THE SOFTWARE.

"""
Data-Parallel Hindley-Milner (DPHM) in sequential Python.
A standalone rendition of the algorithm as presented in the blog post.
https://winwang.blog/posts/dphm-1

There is no let-polymorphism, nor global congruence closure.

Python's loops stand in for bulk parallel operations, which have much harder semantics.
In fact, being sequential basically trivializes bulk_merge.

Meant to be runnable pseudocode.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Literal

# AST expressions


@dataclass(frozen=True)
class Var:
    name: str


@dataclass(frozen=True)
class Lam:  # Lambda but less verbose
    param: str
    body: Expr


@dataclass(frozen=True)
class App:
    fn: Expr
    arg: Expr


@dataclass(frozen=True)
class Const:  # Allowed to be 5 characters because we won't use it
    value: bool | int


Expr = Var | Lam | App | Const

# Types


@dataclass(frozen=True)
class TVar:
    number: int

    def __str__(self) -> str:
        return f"t{self.number}"


@dataclass(frozen=True)
class TAtom:
    name: str

    def __str__(self) -> str:
        return self.name


@dataclass(frozen=True)
class TFn:
    arg: Type  # Although "param" is more accurate, it's too verbose.
    ret: Type

    def __str__(self) -> str:
        arg = f"({self.arg})" if isinstance(self.arg, TFn) else str(self.arg)
        return f"{arg} -> {self.ret}"


Type = TVar | TAtom | TFn

NodeId = int
TypeId = int
# Just imagine that -1 == UINT32_MAX
TypeSentinel = Literal[-1, 0]  # In TInfo.arg: base or unknown, not a child ID.

Equation = tuple[TypeId, TypeId]
BASE_TYPES = ("bool", "int")  # TypeIds 1 and 2; zero is unused or error sentinel.


@dataclass(frozen=True)
class TInfo:
    """One-level type record: arg=-1 is a base, arg=0 an unknown/tvar, else arg->ret."""

    tid: TypeId
    arg: TypeId | TypeSentinel
    ret: TypeId


@dataclass(frozen=True)
class Compiled:
    """All type records and initial constraints, allocated before solving begins."""

    types: tuple[TInfo, ...]  # these are immutable
    initial_constraints: tuple[Equation, ...]
    ast_root_type: TypeId
    # Denote the ID where TFns begin. IDs are assigned like [0 | TAtoms | TVars | TFns].
    fn_start: TypeId


@dataclass(frozen=True)
class CoreResult:
    """The unchanged type records and UF parents (solved equality-class roots)."""

    types: tuple[TInfo, ...]
    roots: tuple[TypeId, ...]


class InferenceError(ValueError):
    pass


# The solver: bulk merge, one Down scan, normalize, repeat.


class Solver:
    def __init__(self, compiled: Compiled) -> None:
        self.problem = compiled
        self.uf = UnionFind(len(compiled.types))

    def run(self) -> CoreResult:
        # A nonempty normalized frontier of constraints joins classes or rejects.
        # No new types are created, so there are only finitely many productive rounds.
        constraints = self.normalize(self.problem.initial_constraints)

        while constraints:
            self.bulk_merge(constraints)
            constraints = self.normalize(self.check_components())

        self.check_occurs()
        return CoreResult(self.problem.types, tuple(self.uf.parent))

    def bulk_merge(self, constraints: tuple[Equation, ...]) -> None:
        """
        Publish a whole batch. This sequential loop models bulk connectivity.
        """
        for left, right in constraints:
            # Earlier links in this batch may change the roots of later pairs.
            # So we must continue to find roots.
            a, b = self.uf.find(left), self.uf.find(right)
            if a == b:
                continue

            # Check for incompatible types
            a_base, b_base = a <= len(BASE_TYPES), b <= len(BASE_TYPES)
            a_fn, b_fn = a >= self.problem.fn_start, b >= self.problem.fn_start
            if a_base and b_base:
                raise InferenceError("distinct base types")
            if (a_base and b_fn) or (a_fn and b_base):
                raise InferenceError("a base type cannot equal a function")

            a_var, b_var = not (a_base or a_fn), not (b_base or b_fn)

            child, parent = a, b

            # A known atom/function outranks an unknown tvar,
            #   i.e. a chosen root should show that its class is rigid.
            if b_var and not a_var:
                child, parent = b, a
            elif a_var and not b_var:
                child, parent = a, b
            else:
                # Among equal shapes, the lower ID wins.
                # I may be regretting this particular choice. It looks weird.
                if a <= b:
                    child, parent = b, a
                else:
                    child, parent = a, b

            self.uf.link_roots(child, parent)

        self.uf.flatten()

    def check_components(self) -> tuple[Equation, ...]:
        """
        Compare each function type with its component's root.
        Then decompose each pair and return the new child equalities.
        """
        # parent array is already flat from bulk_merge, so we can just read it.
        types, parent = self.problem.types, self.uf.parent
        fn_pairs = (
            (types[tid], types[parent[tid]])
            # Only check function types, which are the only ones with children.
            for tid in range(self.problem.fn_start, len(types))
        )

        return tuple(
            eq
            for fn, root in fn_pairs
            if fn != root  # No need to compare a type against itself
            for eq in ((fn.arg, root.arg), (fn.ret, root.ret))
        )

    def normalize(self, equations: tuple[Equation, ...]) -> tuple[Equation, ...]:
        """Map types to current roots, discard satisfied pairs, sort, then dedupe."""
        roots = ((self.uf.find(a), self.uf.find(b)) for a, b in equations)
        # Classic parallel algorithm for deduplication
        pairs = sorted((min(a, b), max(a, b)) for a, b in roots if a != b)
        return tuple(p for i, p in enumerate(pairs) if i == 0 or p != pairs[i - 1])

    def check_occurs(self) -> None:
        """
        Reject cycles in the final class-to-child graph.
        Include every function record, not only the chosen root of each class.

        I'm not even going to check this code because it's boring and it sucks.
        You can tell I didn't write it.
        """
        roots = self.uf.parent
        graph: list[list[TypeId]] = [[] for _ in roots]
        indegree = [0] * len(roots)
        for tid in range(self.problem.fn_start, len(roots)):
            info = self.problem.types[tid]
            source = roots[tid]
            for child in (roots[info.arg], roots[info.ret]):
                graph[source].append(child)
                indegree[child] += 1

        ready = [i for i, degree in enumerate(indegree) if degree == 0]
        visited = 0
        while ready:
            source = ready.pop()
            visited += 1
            for child in graph[source]:
                indegree[child] -= 1
                if indegree[child] == 0:
                    ready.append(child)
        if visited != len(roots):
            raise InferenceError("infinite type")


# Union-find stores equalities; known function structure stays in the arena.


class UnionFind:
    def __init__(self, size: int) -> None:
        self.parent = list(range(size))

    def find(self, tid: TypeId) -> TypeId:
        """
        Find a component's root (find the representative of the equivalence class).
        Flattening can be done explicitly.
        """
        cur = tid
        while self.parent[cur] != cur:
            cur = self.parent[cur]

        return cur

    def link_roots(self, source: TypeId, target: TypeId) -> None:
        """Like a normal union(), except we have already chosen the parent."""
        self.parent[source] = target

    def flatten(self) -> None:
        """Leave every element pointing directly to its root (make components stars)."""
        for tid in range(len(self.parent)):
            self.parent[tid] = self.find(tid)


# The rest of the code from here on is basically just scaffolding.


# Compile every local typing rule before the solver starts.


def compile_expr(expr: Expr) -> Compiled:
    """
    Assign node IDs, then make the lambda/application equations.

    A variable occurrence reuses its binder ID; a constant reuses a base ID.
    Those two typing rules therefore need no separate equations here.
    """
    types = [TInfo(0, 0, 0)]
    types.extend(TInfo(i, -1, i) for i in range(1, len(BASE_TYPES) + 1))

    nodes: list[Expr] = []
    tids: list[TypeId] = []
    children: list[list[NodeId]] = []
    binders: dict[NodeId, TypeId] = {}

    def fresh_var() -> TypeId:
        tid = len(types)
        types.append(TInfo(tid, 0, tid))
        return tid

    # Each occurrence is visited separately, even if the caller reused an AST
    # object. The environment gives each variable its nearest binder's type ID.
    work: list[tuple[Expr, dict[str, TypeId], int | None]] = [(expr, {}, None)]
    while work:
        node, env, parent = work.pop()
        i = len(nodes)
        nodes.append(node)
        children.append([])
        if parent is not None:
            children[parent].append(i)

        if isinstance(node, Var):
            if node.name not in env:
                raise InferenceError(f"unbound variable: {node.name}")
            tid = env[node.name]
        elif isinstance(node, Const):
            if isinstance(node.value, bool):
                tid = 1
            elif type(node.value) is int:
                tid = 2
            else:
                raise InferenceError(f"unsupported constant: {node.value!r}")
        elif isinstance(node, Lam):
            tid = fresh_var()
            binder = fresh_var()
            binders[i] = binder
            work.append((node.body, {**env, node.param: binder}, i))
        elif isinstance(node, App):  # unnecessary but explicit
            tid = fresh_var()
            work.append((node.arg, env, i))
            work.append((node.fn, env, i))
        else:
            raise TypeError(f"unsupported expression: {node!r}")
        tids.append(tid)

    fn_start = len(types)
    equations: list[Equation] = []

    def new_fn(arg: TypeId, ret: TypeId) -> TypeId:
        # No need to intern here, equal-looking arrows can be solved later.
        tid = len(types)
        types.append(TInfo(tid, arg, ret))
        return tid

    for i, node in enumerate(nodes):
        if isinstance(node, Lam):
            # type(lambda x. body) = type(x) -> type(body)
            equations.append((tids[i], new_fn(binders[i], tids[children[i][0]])))
        elif isinstance(node, App):
            # type(fn) = type(arg) -> type(fn arg)
            fn, arg = children[i]
            equations.append((tids[fn], new_fn(tids[arg], tids[i])))

    return Compiled(tuple(types), tuple(equations), tids[0], fn_start)


# Reconstruct just the root type for the public infer() result.


def readback(core: CoreResult, root: TypeId) -> Type:
    """Reconstruct the principal type; reuse a result for each UF class."""
    arena, roots = core.types, core.roots
    memo: dict[TypeId, Type] = {}
    stack = [(roots[root], False)]
    next_var = 0

    while stack:
        cls, assemble = stack.pop()
        if cls in memo:
            continue
        info = arena[cls]
        if info.arg == -1:
            memo[cls] = TAtom(BASE_TYPES[cls - 1])
        elif info.arg == 0:
            memo[cls] = TVar(next_var)
            next_var += 1
        elif assemble:
            memo[cls] = TFn(memo[roots[info.arg]], memo[roots[info.ret]])
        else:
            stack.append((cls, True))
            stack.append((roots[info.ret], False))
            stack.append((roots[info.arg], False))
    return memo[roots[root]]


# Public entrypoint


def infer(expr: Expr) -> Type:
    compiled = compile_expr(expr)
    return readback(Solver(compiled).run(), compiled.ast_root_type)


if __name__ == "__main__":
    identity = Lam("x", Var("x"))
    assert infer(App(identity, Const(5))) == TAtom("int")
    print(f"identity: {infer(identity)}")

    # Both child equalities emerge by successive one-level Down scans.
    # \f. \x. (f x) ((\y. y) x)
    bridge = Lam(
        "f", Lam("x", App(App(Var("f"), Var("x")), App(Lam("y", Var("y")), Var("x"))))
    )
    print(f"bridge:   {infer(bridge)}")

    for bad in (Lam("x", App(Var("x"), Var("x"))), App(Const(5), Const(5))):
        try:
            infer(bad)
        except InferenceError:
            pass
        else:
            raise AssertionError(f"accepted ill-typed term: {bad!r}")
    print("ok")
