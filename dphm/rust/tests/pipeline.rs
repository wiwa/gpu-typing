// Generated/maintained with AI; see repository disclaimers.md.
use dphm::{compile, readback, sequential, unary, Ast, Error, Node};

fn identity() -> Ast {
    Ast {
        nodes: vec![Node::Lam(1), Node::Var(0, 0)],
        root: 0,
    }
}

#[test]
fn all_node_and_binder_types_share_the_same_unknown() {
    let p = compile(&identity()).unwrap();
    let source = unary::ResolvedAst::new(&identity()).unwrap();
    let types = unary::infer_types(&source).unwrap();
    let lambda = types.node(0).unwrap();
    let occurrence = types.node(1).unwrap();
    let parameter = types.binder(0).unwrap();
    assert_eq!(lambda.arg1, types.source[0][1]);
    assert_eq!(lambda.ret, types.source[0][1]);
    assert_eq!(occurrence, parameter);
    assert_eq!(types.get(types.source[0][0]), Some(lambda));
    assert_eq!(types.binder(1), None);
    assert_eq!(types.node(2), None);
    assert_eq!(
        readback::depth(&p, &sequential::solve(&p).unwrap()).unwrap(),
        1
    );
    assert_eq!(types.get(0), None);
    assert_eq!(types.get(u32::MAX), None);
}

#[test]
fn malformed_asts_are_rejected() {
    let bad = [
        Ast {
            nodes: vec![],
            root: 0,
        },
        Ast {
            nodes: vec![Node::Base(1)],
            root: 1,
        },
        Ast {
            nodes: vec![Node::Base(3)],
            root: 0,
        },
        Ast {
            nodes: vec![Node::Var(0, 0)],
            root: 0,
        },
        Ast {
            nodes: vec![Node::Lam(4)],
            root: 0,
        },
        Ast {
            nodes: vec![Node::Lam(0)],
            root: 0,
        },
        Ast {
            nodes: vec![Node::App(1, 1), Node::Base(1)],
            root: 0,
        },
        Ast {
            nodes: vec![Node::Base(1), Node::Base(2)],
            root: 0,
        },
        // The lambda in the function child does not bind the argument sibling.
        Ast {
            nodes: vec![
                Node::App(1, 3),
                Node::Lam(2),
                Node::Var(1, 0),
                Node::Var(1, 0),
            ],
            root: 0,
        },
    ];
    for ast in bad {
        assert!(
            matches!(compile(&ast), Err(Error::InvalidAst(_))),
            "{ast:?}"
        );
    }
}

#[test]
fn compiler_and_readback_handle_deep_asts_iteratively() {
    let depth = 10_000;
    let mut nodes: Vec<_> = (1..=depth).map(|i| Node::Lam(i as u32)).collect();
    nodes.push(Node::Base(2));
    let p = compile(&Ast { nodes, root: 0 }).unwrap();
    let labels = sequential::solve(&p).unwrap();
    assert_eq!(readback::depth(&p, &labels).unwrap(), depth as u32);
    assert_eq!(p.node_types.len(), depth + 1);
    assert_eq!(
        p.binder_types.iter().filter(|&&ids| ids[0] != 0).count(),
        depth
    );
}
