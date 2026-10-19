// Generated/maintained with AI; see repository disclaimers.md.
mod common;
use dphm::{
    unary::{self, ResolvedAst, Typing},
    Ast, Node,
};

fn check(source: &ResolvedAst, actual: &Typing, labels: &[u32]) {
    let p = source.compile();
    assert_eq!(actual.source.len(), p.node_types.len());
    assert_eq!(actual.arrows.len(), source.equation_count());
    for (i, &[node, binder]) in actual.source.iter().enumerate() {
        assert_eq!(node, labels[p.node_types[i] as usize]);
        assert_eq!(binder, labels[p.binder_types[i][0] as usize]);
        assert!(actual.node(i as u32).is_some());
        assert_eq!(actual.binder(i as u32).is_some(), binder != 0);
    }
    for (t, children) in p.arena[actual.first_arrow as usize..]
        .iter()
        .zip(&actual.arrows)
    {
        assert_eq!(*children, [labels[t.arg1 as usize], labels[t.ret as usize]]);
    }
    assert!(actual.node(actual.root_node).is_some());
    assert!(actual.get(0).is_none());
    assert!(actual.get(source.type_count() as u32).is_none());
}

#[test]
fn streamed_eager_typings_match_full_diagnostics() {
    for (ast, error) in common::cases() {
        let source = ResolvedAst::new(&ast).unwrap();
        let full = unary::infer(&source);
        assert_eq!(full.as_ref().err(), error.as_ref());
        match (full, unary::infer_types(&source)) {
            (Ok(full), Ok(types)) => check(&source, &types, &full.labels),
            (Err(a), Err(b)) => assert_eq!(a, b),
            _ => panic!("streamed eager solver disagrees"),
        }
    }
    // Both occurrences and the binder share one variable, not renamed per query.
    let source = ResolvedAst::new(&Ast {
        nodes: vec![Node::Lam(1), Node::Var(0, 0)],
        root: 0,
    })
    .unwrap();
    let types = unary::infer_types(&source).unwrap();
    assert_eq!(types.source[1][0], types.source[0][1]);
    let function = types.node(0).unwrap();
    assert_eq!(function.arg1, function.ret);
    assert_eq!(function.arg1, types.source[1][0]);
}
