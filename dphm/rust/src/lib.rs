// Generated/maintained with AI; see repository disclaimers.md.
//! Closed STLC on flat arrays. Source IDs survive compilation and inference.
//!
//! Infer `\x. x` with Algorithm J, then query its node and binder types:
//!
//! ```
//! use dphm::{j, unary::ResolvedAst, Ast, Node};
//!
//! let ast = Ast {
//!     nodes: vec![Node::Lam(1), Node::Var(0, 0)],
//!     root: 0,
//! };
//! let source = ResolvedAst::new(&ast)?;
//! let typing = j::infer_types(&source)?;
//!
//! let arrow = typing.node(0).unwrap();
//! let parameter = typing.source[0][1]; // The lambda's retained binder type ID.
//! assert_eq!((arrow.arg1, arrow.ret), (parameter, parameter));
//! assert_eq!(typing.source[1][0], parameter); // x has that same type.
//! assert!(typing.binder(0).is_some());
//! # Ok::<(), dphm::Error>(())
//! ```

#[cfg(feature = "cuda")]
pub mod cuda;
pub mod j;
pub mod j_compact;
pub mod readback;
pub mod sequential;
pub mod unary;

use std::fmt;

pub type TypeId = u32;
/// Base constructor marker in TInfo.arg1; never a valid type ID.
pub const BASE_ARG: TypeId = TypeId::MAX;

/// Var names its enclosing lambda node; Lam/App name their child nodes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Node {
    Var(u32, u32), // Lambda node and reserved parameter index (always zero).
    Lam(u32),
    App(u32, u32),
    Base(u32),
}

#[derive(Debug)]
pub struct Ast {
    pub nodes: Vec<Node>,
    pub root: u32,
}

/// One type level: arg1=BASE_ARG is a base, arg1=0 a variable, otherwise an arrow.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TInfo {
    pub arg1: TypeId,
    pub ret: TypeId,
}

impl TInfo {
    pub fn is_arrow(self) -> bool {
        self.arg1 != 0 && self.arg1 != BASE_ARG
    }

    pub fn is_base(self) -> bool {
        self.arg1 == BASE_ARG
    }

    pub fn children(self) -> [TypeId; 2] {
        if self.is_arrow() {
            [self.arg1, self.ret]
        } else {
            [0, 0]
        }
    }

    pub fn signature(self, labels: &[TypeId]) -> [TypeId; 2] {
        [labels[self.arg1 as usize], labels[self.ret as usize]]
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Equation {
    pub left: TypeId,
    pub right: TypeId,
}

pub struct Problem {
    pub arena: Vec<TInfo>,
    pub equations: Vec<Equation>,
    /// Number of compiled arrows, which occupy the arena's final contiguous range.
    pub arrow_capacity: Option<usize>,
    /// Indexed by source node ID. Variable uses and base literals may alias.
    pub node_types: Vec<TypeId>,
    /// Parameter type at each lambda: [type_id, 0]; non-lambdas use [0, 0].
    pub binder_types: Vec<[TypeId; 2]>,
    pub root: TypeId,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    InvalidAst(String),
    InvalidProblem(&'static str),
    RigidConflict,
    InfiniteType,
    Capacity(&'static str),
    Cuda(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidAst(s) | Self::Cuda(s) => f.write_str(s),
            Self::InvalidProblem(s) | Self::Capacity(s) => f.write_str(s),
            Self::RigidConflict => f.write_str("rigid_conflict"),
            Self::InfiniteType => f.write_str("infinite_type"),
        }
    }
}

impl std::error::Error for Error {}

/// Check source scope and emit local unary typing equations.
pub fn compile(ast: &Ast) -> Result<Problem, Error> {
    Ok(unary::ResolvedAst::new(ast)?.compile())
}

pub fn validate_problem(p: &Problem) -> Result<(), Error> {
    let rows = p.arena.len();
    let valid = |id: TypeId| id > 0 && (id as usize) < rows;

    if rows < 3 || rows - 1 > i32::MAX as usize / 12 {
        return Err(Error::InvalidProblem("invalid arena size"));
    }

    for (i, t) in p.arena.iter().enumerate() {
        let shape = match i {
            0 => t.arg1 == 0 && t.ret == 0,
            1 | 2 => t.arg1 == crate::BASE_ARG && t.ret == i as TypeId,
            _ => (t.arg1 == 0 && t.ret == i as TypeId) || (valid(t.arg1) && valid(t.ret)),
        };
        if !shape {
            return Err(Error::InvalidProblem("invalid TInfo"));
        }
    }

    if p.equations
        .iter()
        .any(|e| !valid(e.left) || !valid(e.right))
    {
        return Err(Error::InvalidProblem("invalid equation"));
    }

    Ok(())
}

#[cfg(test)]
mod representation_tests {
    use super::*;
    use std::mem::{align_of, size_of};

    #[test]
    fn unsigned_abi_and_unary_markers() {
        assert_eq!((size_of::<TInfo>(), align_of::<TInfo>()), (8, 4));
        assert_eq!((size_of::<Equation>(), align_of::<Equation>()), (8, 4));
        for (arg1, base, arrow) in [(0, false, false), (BASE_ARG, true, false), (1, false, true)] {
            let t = TInfo { arg1, ret: 1 };
            assert_eq!((t.is_base(), t.is_arrow()), (base, arrow));
        }
    }

    #[test]
    fn high_bit_ids_and_misplaced_base_markers_are_rejected() {
        let ast = Ast {
            nodes: vec![Node::Lam(1), Node::Var(0, 0)],
            root: 0,
        };
        for invalid in [1 << 31, BASE_ARG - 1, BASE_ARG] {
            for field in 0..4 {
                let mut p = compile(&ast).unwrap();
                match field {
                    0 => p.arena[3].arg1 = invalid,
                    1 => p.arena[3].ret = invalid,
                    2 => p.equations[0].left = invalid,
                    _ => p.equations[0].right = invalid,
                }
                assert!(validate_problem(&p).is_err());
            }
        }
    }
}
