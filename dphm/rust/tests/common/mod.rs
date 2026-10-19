// Generated/maintained with AI; see repository disclaimers.md.
// Integration test binaries use different subsets of these shared fixtures.
#![allow(dead_code)]
use dphm::{unary::Typing, Ast, Error, Node};
use std::collections::{HashMap, HashSet};

// Compare every source/binder type in one shared variable namespace, without
// requiring the engines to choose the same IDs or equal-arrow representation.
pub fn equivalent(a: &Typing, b: &Typing) {
    assert_eq!(a.root_node, b.root_node);
    assert_eq!(a.source.len(), b.source.len());
    assert_eq!(a.arrows.len(), b.arrows.len());
    let (mut forward, mut backward, mut seen) = (HashMap::new(), HashMap::new(), HashSet::new());
    let mut pending: Vec<_> = a
        .source
        .iter()
        .zip(&b.source)
        .flat_map(|(a, b)| [(a[0], b[0]), (a[1], b[1])])
        .collect();
    while let Some((x, y)) = pending.pop() {
        assert_eq!(x == 0, y == 0);
        if x == 0 || !seen.insert((x, y)) {
            continue;
        }
        let (tx, ty) = (a.get(x).unwrap(), b.get(y).unwrap());
        assert_eq!((tx.is_arrow(), tx.is_base()), (ty.is_arrow(), ty.is_base()));
        if tx.is_arrow() {
            pending.extend([(tx.arg1, ty.arg1), (tx.ret, ty.ret)]);
        } else if tx.is_base() {
            assert_eq!(x, y);
        } else {
            assert_eq!(*forward.entry(x).or_insert(y), y);
            assert_eq!(*backward.entry(y).or_insert(x), x);
        }
    }
}

// Tiny known-answer fixtures only: fully parenthesized arrows, variables named
// in first-occurrence order. Large/shared results use equivalent(), not expansion.
pub fn assert_root_type(typing: &Typing, expected: &str) {
    fn show(typing: &Typing, id: u32, variables: &mut HashMap<u32, usize>) -> String {
        let t = typing.get(id).unwrap();
        if t.is_arrow() {
            format!(
                "({} -> {})",
                show(typing, t.arg1, variables),
                show(typing, t.ret, variables)
            )
        } else if t.is_base() {
            if id == 1 { "Bool" } else { "Int" }.into()
        } else {
            let next = variables.len();
            format!("t{}", variables.entry(id).or_insert(next))
        }
    }
    assert_eq!(
        show(
            typing,
            typing.source[typing.root_node as usize][0],
            &mut HashMap::new()
        ),
        expected
    );
}

pub fn known_types() -> Vec<(Ast, &'static str)> {
    use Node::{App, Base, Lam, Var};
    [
        (vec![Lam(1), Var(0, 0)], "(t0 -> t0)"),                 // I
        (vec![Lam(1), Lam(2), Var(0, 0)], "(t0 -> (t1 -> t0))"), // K
        (
            vec![
                Lam(1),
                Lam(2),
                Lam(3),
                App(4, 7),
                App(5, 6),
                Var(0, 0),
                Var(2, 0),
                App(8, 9),
                Var(1, 0),
                Var(2, 0),
            ],
            "((t0 -> (t1 -> t2)) -> ((t0 -> t1) -> (t0 -> t2)))",
        ), // S
        (
            vec![
                Lam(1),
                Lam(2),
                Lam(3),
                App(4, 5),
                Var(0, 0),
                App(6, 7),
                Var(1, 0),
                Var(2, 0),
            ],
            "((t0 -> t1) -> ((t2 -> t0) -> (t2 -> t1)))",
        ), // B: composition
        (
            vec![
                Lam(1),
                Lam(2),
                Lam(3),
                App(4, 7),
                App(5, 6),
                Var(0, 0),
                Var(2, 0),
                Var(1, 0),
            ],
            "((t0 -> (t1 -> t2)) -> (t1 -> (t0 -> t2)))",
        ), // C: flip
        (
            vec![
                Lam(1),
                Lam(2),
                App(3, 4),
                Var(0, 0),
                App(5, 6),
                Var(0, 0),
                Var(1, 0),
            ],
            "((t0 -> t0) -> (t0 -> t0))",
        ), // twice / Church 2
        (vec![Lam(1), Lam(2), Var(1, 0)], "(t0 -> (t1 -> t1))"), // Church 0
        (vec![App(1, 3), Lam(2), Var(1, 0), Base(2)], "Int"),
    ]
    .into_iter()
    .map(|(nodes, ty)| (Ast { nodes, root: 0 }, ty))
    .collect()
}

// Balanced discard applications keep many identity applications to the same
// outer x live in the input, without imposing a long source/type-depth chain.
pub fn shared_binder(leaves: usize) -> Ast {
    assert!(leaves.is_power_of_two());
    let mut nodes = vec![Node::Lam(0)];
    let mut roots = Vec::new();
    for _ in 0..leaves {
        let i = nodes.len() as u32;
        nodes.extend([
            Node::App(i + 1, i + 3),
            Node::Lam(i + 2),
            Node::Var(i + 1, 0),
            Node::Var(0, 0),
        ]);
        roots.push(i);
    }
    while roots.len() > 1 {
        roots = roots
            .chunks(2)
            .map(|pair| {
                let i = nodes.len() as u32;
                nodes.extend([Node::App(i + 1, pair[0]), Node::Lam(pair[1])]);
                i
            })
            .collect();
    }
    nodes[0] = Node::Lam(roots[0]);
    Ast { nodes, root: 0 }
}

// Small closed terms, including unused binders and arbitrary outer references.
pub fn random_term(n: usize, seed: &mut u64, nodes: &mut Vec<Node>, scope: &mut Vec<u32>) -> u32 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
    let choice = (*seed >> 32) as usize;
    let i = nodes.len() as u32;
    nodes.push(Node::Base(1));
    nodes[i as usize] = if n == 1 {
        if choice % (scope.len() + 2) < scope.len() {
            Node::Var(scope[choice % (scope.len() + 2)], 0)
        } else {
            Node::Base(1 + (choice % 2) as u32)
        }
    } else if n == 2 || choice & 1 == 0 {
        scope.push(i);
        let body = random_term(n - 1, seed, nodes, scope);
        scope.pop();
        Node::Lam(body)
    } else {
        let left = 1 + choice % (n - 2);
        let f = random_term(left, seed, nodes, scope);
        let x = random_term(n - 1 - left, seed, nodes, scope);
        Node::App(f, x)
    };
    i
}

fn deep(depth: usize) -> Ast {
    let mut nodes: Vec<_> = (1..=depth).map(|i| Node::Lam(i as u32)).collect();
    nodes.push(Node::Base(2));
    Ast { nodes, root: 0 }
}

pub fn cases() -> Vec<(Ast, Option<Error>)> {
    use Node::{App, Base, Lam, Var};
    let mut cases: Vec<_> = [
        (vec![Base(1)], None),
        (vec![App(1, 3), Lam(2), Var(1, 0), Base(2)], None),
        (vec![Lam(1), Var(0, 0)], None),
        (vec![Lam(1), Lam(2), Var(0, 0)], None), // distinct unknowns
        (
            vec![App(1, 2), Base(2), Base(2)],
            Some(Error::RigidConflict),
        ),
        (
            vec![Lam(1), App(2, 3), Var(0, 0), Var(0, 0)],
            Some(Error::InfiniteType),
        ),
        // Root is Int, but its unused argument has an indirect occurs cycle.
        (
            vec![
                App(1, 3),
                Lam(2),
                Base(2),
                Lam(4),
                Lam(5),
                App(6, 9),
                App(7, 8),
                Var(3, 0),
                Var(4, 0),
                App(10, 11),
                Var(4, 0),
                Var(3, 0),
            ],
            Some(Error::InfiniteType),
        ),
    ]
    .into_iter()
    .map(|(nodes, error)| (Ast { nodes, root: 0 }, error))
    .collect();
    // Reuse after rejection, reversed storage and a nonzero root.
    cases.push((
        Ast {
            nodes: vec![Var(1, 0), Lam(0)],
            root: 1,
        },
        None,
    ));
    cases.push((deep(64), None));
    cases
}
