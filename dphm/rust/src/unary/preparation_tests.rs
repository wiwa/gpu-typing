// Generated/maintained with AI; see repository disclaimers.md.
//! Compare packed syntax and scope checking with a separate two-pass reference.
use super::*;

// Test-only reference, never used by inference.
fn two_pass(ast: &Ast) -> Result<ResolvedAst, Error> {
    let n = ast.nodes.len();
    if n == 0 || n > i32::MAX as usize / 12 || ast.root as usize >= n {
        return Err(Error::InvalidAst("empty or oversized AST/root".into()));
    }
    let mut nodes = Vec::with_capacity(n);
    let (mut variables, mut arrows) = (0, 0);
    for node in &ast.nodes {
        let (tag, a, b) = match *node {
            Node::Var(binder, 0) => (0, binder, 0),
            Node::Lam(body) => {
                variables += 2;
                arrows += 1;
                (1, body, 0)
            }
            Node::App(f, x) => {
                variables += 1;
                arrows += 1;
                (2, f, x)
            }
            Node::Base(base @ (1 | 2)) => (3, base, 0),
            _ => return Err(Error::InvalidAst("expected unary STLC".into())),
        };
        nodes.push(SourceNode { tag, a, b });
    }
    if 3 + variables + arrows - 1 > i32::MAX as usize / 12 {
        return Err(Error::Capacity("unary arena exceeds 32-bit frontier limit"));
    }
    let mut seen = vec![false; n];
    let mut active = vec![false; n];
    let mut work = vec![(ast.root as usize, false)];
    while let Some((i, leaving)) = work.pop() {
        let node = nodes
            .get(i)
            .ok_or_else(|| Error::InvalidAst("child out of range".into()))?;
        if leaving {
            active[i] = false;
            continue;
        }
        if seen[i] {
            return Err(Error::InvalidAst("AST must be a tree".into()));
        }
        seen[i] = true;
        match node.tag {
            0 if !active.get(node.a as usize).copied().unwrap_or(false) => {
                return Err(Error::InvalidAst(
                    "variable outside its lexical scope".into(),
                ))
            }
            1 => {
                active[i] = true;
                work.push((i, true));
                work.push((node.a as usize, false));
            }
            2 => {
                work.push((node.b as usize, false));
                work.push((node.a as usize, false));
            }
            _ => (),
        }
    }
    if seen.iter().any(|s| !s) {
        return Err(Error::InvalidAst("unreachable AST nodes".into()));
    }
    Ok(ResolvedAst {
        nodes,
        variables,
        arrows,
        root: ast.root as usize,
    })
}

fn same(a: &ResolvedAst, b: &ResolvedAst) {
    assert_eq!(a.nodes, b.nodes);
    assert_eq!(
        (a.root, a.variables, a.arrows),
        (b.root, b.variables, b.arrows)
    );
}

#[test]
fn packing_and_rejection_match_two_pass() {
    // Exhaust all small flat graphs, including shared/cyclic children, dangling
    // and out-of-scope variables, non-lambda binders and unreachable nodes.
    for n in 1..=3 {
        let mut choices = vec![Node::Base(0), Node::Base(1), Node::Base(2), Node::Base(3)];
        for a in 0..=n as u32 {
            choices.extend([Node::Var(a, 0), Node::Var(a, 1), Node::Lam(a)]);
            for b in 0..=n as u32 {
                choices.push(Node::App(a, b));
            }
        }
        for mut code in 0..choices.len().pow(n as u32) {
            let nodes = (0..n)
                .map(|_| {
                    let node = choices[code % choices.len()];
                    code /= choices.len();
                    node
                })
                .collect();
            let mut ast = Ast { nodes, root: 0 };
            for root in 0..=n as u32 {
                ast.root = root;
                match (two_pass(&ast), ResolvedAst::new(&ast)) {
                    (Ok(a), Ok(b)) => same(&a, &b),
                    (Err(_), Err(_)) => (),
                    _ => panic!("acceptance differs: {ast:?}"),
                }
            }
        }
    }
    assert!(ResolvedAst::new(&Ast {
        nodes: vec![],
        root: 0
    })
    .is_err());
    // The lambda has already exited when its sibling variable is visited.
    let outside_sibling = Ast {
        nodes: vec![
            Node::App(1, 3),
            Node::Lam(2),
            Node::Base(1),
            Node::Var(1, 0),
        ],
        root: 0,
    };
    assert!(two_pass(&outside_sibling).is_err());
    assert!(ResolvedAst::new(&outside_sibling).is_err());
}
