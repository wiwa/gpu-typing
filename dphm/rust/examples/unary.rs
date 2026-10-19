// Generated/maintained with AI; see repository disclaimers.md.
//! Apply \x. x to an Int literal, then query types without expanding trees.
use dphm::{unary, Ast, Node};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if !args.is_empty() && args != ["--cuda"] && args != ["--cuda-b"] {
        return Err("usage: unary [--cuda | --cuda-b]".into());
    }
    let ast = Ast {
        nodes: vec![
            Node::App(1, 3),
            Node::Lam(2),
            Node::Var(1, 0),
            Node::Base(2),
        ],
        root: 0,
    };
    let source = unary::ResolvedAst::new(&ast)?;
    let typing = if !args.is_empty() {
        #[cfg(feature = "cuda")]
        {
            use dphm::cuda::{Checker, Workspace};
            let mut workspace = Workspace::new()?;
            let checker = if args == ["--cuda-b"] {
                Checker::B
            } else {
                Checker::A
            };
            workspace.infer_ast(&source, checker, false).0?
        }
        #[cfg(not(feature = "cuda"))]
        return Err("rebuild with --features cuda to select the GPU".into());
    } else {
        unary::infer_types(&source)?
    };
    assert_eq!(typing.source[typing.root_node as usize][0], 2);
    assert_eq!(typing.source[1][1], typing.source[2][0]);
    println!(
        "root: Int; types available for all {} source nodes",
        typing.source.len()
    );
    println!("[node type, binder type] references: {:?}", typing.source);
    Ok(())
}
