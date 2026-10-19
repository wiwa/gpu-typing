// Generated/maintained with AI; see repository disclaimers.md.
mod common;
use common::equivalent;
use dphm::{
    j, j_compact,
    unary::{self, ResolvedAst},
    Ast, Node,
};

#[test]
fn j_engines_match_fixture_types_and_rejections() {
    for (ast, error) in common::cases() {
        let source = ResolvedAst::new(&ast).unwrap();
        for solve in [j::infer_types, j_compact::infer_types] {
            let actual = solve(&source);
            assert_eq!(actual.as_ref().err(), error.as_ref());
            if let Ok(actual) = actual {
                equivalent(&unary::infer_types(&source).unwrap(), &actual);
            }
        }
    }
}

#[test]
fn j_engines_match_eager_on_random_closed_terms() {
    let mut seed = 7;
    let mut successes = 0;
    for n in 1..=64 {
        for _ in 0..128 {
            let mut ast = Ast {
                nodes: Vec::new(),
                root: 0,
            };
            common::random_term(n, &mut seed, &mut ast.nodes, &mut Vec::new());
            let source = ResolvedAst::new(&ast).unwrap();
            let expected = unary::infer_types(&source);
            successes += usize::from(expected.is_ok());
            for solve in [j::infer_types, j_compact::infer_types] {
                match (&expected, solve(&source)) {
                    (Ok(a), Ok(b)) => {
                        equivalent(a, &b);
                    }
                    (Err(_), Err(_)) => (), // Error priority may differ with traversal order.
                    _ => panic!("acceptance mismatch: {ast:?}"),
                }
            }
        }
    }
    assert!(successes > 500);
}

#[test]
fn cpu_engines_match_known_principal_types_and_shared_binder() {
    let mut cases = common::known_types();
    cases.push((common::shared_binder(1024), "(t0 -> t0)"));
    for (ast, ty) in cases {
        let source = ResolvedAst::new(&ast).unwrap();
        for solve in [unary::infer_types, j::infer_types, j_compact::infer_types] {
            common::assert_root_type(&solve(&source).unwrap(), ty);
        }
    }
}

#[test]
fn j_engines_handle_deep_trees_without_recursion() {
    let depth = 20_000;
    let mut nodes: Vec<_> = (1..=depth).map(Node::Lam).collect();
    nodes.push(Node::Base(2));
    let source = ResolvedAst::new(&Ast { nodes, root: 0 }).unwrap();
    for solve in [j::infer_types, j_compact::infer_types] {
        equivalent(
            &unary::infer_types(&source).unwrap(),
            &solve(&source).unwrap(),
        );
    }
}

#[test]
fn known_function_applications_preserve_all_source_types() {
    // id (id (... Int)): every function head is known before its application.
    let depth = 10_000;
    let mut nodes = Vec::with_capacity(3 * depth + 1);
    for i in 0..depth as u32 {
        let at = 3 * i;
        nodes.extend([
            Node::App(at + 1, at + 3),
            Node::Lam(at + 2),
            Node::Var(at + 1, 0),
        ]);
    }
    nodes.push(Node::Base(2));
    let source = ResolvedAst::new(&Ast { nodes, root: 0 }).unwrap();
    let expected = unary::infer_types(&source).unwrap();
    for solve in [j::infer_types, j_compact::infer_types] {
        equivalent(&expected, &solve(&source).unwrap());
    }
}
