// Generated/maintained with AI; see repository disclaimers.md.
#![cfg(feature = "cuda")]

mod common;
use common::equivalent;
use dphm::{
    cuda::{Checker, Workspace},
    j_compact,
    unary::{self, ResolvedAst},
    Ast,
};

#[test]
fn gpu_matches_known_types_and_repeats_concentrated_merges() {
    let mut cases = common::known_types();
    // 98,303 nodes, all identity arguments refer to the same outer binder.
    cases.push((common::shared_binder(16_384), "(t0 -> t0)"));
    let mut workspace = Workspace::new().unwrap();
    for (ast, ty) in cases {
        let source = ResolvedAst::new(&ast).unwrap();
        let expected = j_compact::infer_types(&source).unwrap();
        common::assert_root_type(&expected, ty);
        for checker in [Checker::A, Checker::B] {
            let mut previous = None;
            for _ in 0..4 {
                let actual = workspace.infer_ast(&source, checker, false).0.unwrap();
                common::assert_root_type(&actual, ty);
                equivalent(&expected, &actual);
                if let Some((arrows, source)) = &previous {
                    assert_eq!(&actual.arrows, arrows);
                    assert_eq!(&actual.source, source);
                }
                previous = Some((actual.arrows, actual.source));
            }
        }
    }
}

#[test]
fn gpu_matches_compact_j_on_random_closed_terms() {
    let mut seed = 7;
    let mut successes = 0;
    let mut workspace = Workspace::new().unwrap();
    for n in 1..=32 {
        for _ in 0..8 {
            let mut ast = Ast {
                nodes: Vec::new(),
                root: 0,
            };
            common::random_term(n, &mut seed, &mut ast.nodes, &mut Vec::new());
            let source = ResolvedAst::new(&ast).unwrap();
            let expected = j_compact::infer_types(&source);
            successes += usize::from(expected.is_ok());
            for checker in [Checker::A, Checker::B] {
                let actual = workspace.infer_ast(&source, checker, false).0;
                match (&expected, actual) {
                    (Ok(left), Ok(right)) => equivalent(left, &right),
                    (Err(_), Err(dphm::Error::RigidConflict | dphm::Error::InfiniteType)) => (),
                    _ => panic!("CPU/GPU acceptance mismatch: {ast:?}"),
                }
            }
        }
    }
    assert!(successes > 20);
}

#[test]
fn whole_tree_gpu_typings_match_eager_cpu_and_recover_after_indirect_cycle() {
    let mut workspace = Workspace::new().unwrap();
    for (i, (ast, error)) in common::cases().into_iter().enumerate() {
        let source = ResolvedAst::new(&ast).unwrap();
        let expected = unary::infer_types(&source);
        assert_eq!(expected.as_ref().err(), error.as_ref());
        for checker in [Checker::A, Checker::B] {
            for instrument in [false, true] {
                let (actual, metrics) = workspace.infer_ast(&source, checker, instrument);
                assert_eq!(actual.as_ref().err(), error.as_ref());
                assert_eq!(metrics.graph_launches, 1);
                assert_eq!(metrics.synchronizations, 2);
                assert!(workspace.export_ms.unwrap() >= 0.0);
                assert!(
                    workspace.source_device_ms.unwrap()
                        >= workspace.inference_ms.unwrap() + workspace.export_ms.unwrap() - 0.01
                );
                match (&expected, actual) {
                    (Ok(left), Ok(right)) => equivalent(left, &right),
                    (Err(left), Err(right)) => assert_eq!(*left, right),
                    _ => panic!("whole-tree CPU/GPU inference disagrees in case {i}"),
                }
            }
        }
    }
}

#[test]
fn large_registered_outputs_reject_recover_and_remain_rust_owned() {
    use dphm::{Ast, Node};
    fn broad(infinite: bool) -> Ast {
        let mut nodes = Vec::new();
        let mut roots = Vec::new();
        for i in 0..16_384 {
            let lambda = nodes.len() as u32;
            nodes.push(Node::Lam(0));
            let variable = nodes.len() as u32;
            nodes.push(Node::Var(lambda, 0));
            let body = if infinite && i == 0 {
                nodes.push(Node::Var(lambda, 0));
                let body = nodes.len() as u32;
                nodes.push(Node::App(variable, variable + 1));
                body
            } else {
                variable
            };
            nodes[lambda as usize] = Node::Lam(body);
            if infinite && i == 0 {
                roots.push(lambda); // Keep x unconstrained so this tests occurs rejection.
                continue;
            }
            let base = nodes.len() as u32;
            nodes.push(Node::Base(2));
            let application = nodes.len() as u32;
            nodes.push(Node::App(lambda, base));
            roots.push(application);
        }
        while roots.len() > 1 {
            roots = roots
                .chunks(2)
                .map(|pair| {
                    let lambda = nodes.len() as u32;
                    nodes.push(Node::Lam(pair[1]));
                    let application = nodes.len() as u32;
                    nodes.push(Node::App(lambda, pair[0]));
                    application
                })
                .collect();
        }
        Ast {
            nodes,
            root: roots[0],
        }
    }
    let good = ResolvedAst::new(&broad(false)).unwrap();
    let bad = ResolvedAst::new(&broad(true)).unwrap();
    let expected = unary::infer_types(&good).unwrap();
    let mut workspace = Workspace::new().unwrap();
    for registered in [true, false, true] {
        workspace.register_output = registered;
        let (result, metrics) = workspace.infer_ast(&good, Checker::A, false);
        let mut result = result.unwrap();
        equivalent(&expected, &result);
        let bytes = 8 * (result.source.len() + result.arrows.len());
        assert!(bytes >= 1024 * 1024);
        assert!(
            metrics.registered_output_bytes == 0
                || (registered && metrics.registered_output_bytes == bytes as u64)
        );
        // Successful delivery leaves ordinary owned vectors, usable after unpinning.
        result.source.push([0, 0]);
        result.arrows.push([0, 0]);
        let (rejected, _) = workspace.infer_ast(&bad, Checker::A, false);
        assert_eq!(rejected.err(), Some(dphm::Error::InfiniteType));
    }
}
