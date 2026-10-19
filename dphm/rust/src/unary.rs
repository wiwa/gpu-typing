// Generated/maintained with AI; see repository disclaimers.md.
//! Resolved unary STLC. Lowering is a prefix sum and one local emission per node.
use crate::{Ast, Equation, Error, Node, Problem, TInfo, TypeId};

/// Wire format shared with CUDA: 0=Var(binder,0), 1=Lam(body), 2=App(f,x), 3=Base(id).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SourceNode {
    pub tag: u32,
    pub a: u32,
    pub b: u32,
}

/// A checked tree with resolved binder references; no type IDs or constraints yet.
pub struct ResolvedAst {
    pub(crate) nodes: Vec<SourceNode>,
    pub(crate) variables: usize,
    pub(crate) arrows: usize,
    pub(crate) root: usize,
}

impl ResolvedAst {
    /// Validate syntax/scope and pack the AST, without inferring or assigning types.
    /// A parser/name resolver may prepare this once for any inference engine.
    pub fn new(ast: &Ast) -> Result<Self, Error> {
        let n = ast.nodes.len();
        if n == 0 || n > i32::MAX as usize / 12 || ast.root as usize >= n {
            return Err(Error::InvalidAst("empty or oversized AST/root".into()));
        }

        let mut nodes = Box::<[SourceNode]>::new_uninit_slice(n);
        let (mut variables, mut arrows) = (0, 0);
        // 0=unseen, 1=active lambda, 2=visited (not an active binder).
        let mut state = vec![0u8; n];
        let mut visited = 0;
        let mut work = vec![(ast.root as usize, false)];

        while let Some((i, leaving)) = work.pop() {
            let node = ast
                .nodes
                .get(i)
                .ok_or_else(|| Error::InvalidAst("child out of range".into()))?;

            if leaving {
                state[i] = 2;
                continue;
            }

            if state[i] != 0 {
                return Err(Error::InvalidAst("AST must be a tree".into()));
            }

            state[i] = 2;
            visited += 1;
            let (tag, a, b) = match *node {
                Node::Var(binder, 0) if state.get(binder as usize) != Some(&1) => {
                    return Err(Error::InvalidAst(
                        "variable outside its lexical scope".into(),
                    ))
                }
                Node::Var(binder, 0) => (0, binder, 0),
                Node::Lam(body) => {
                    variables += 2;
                    arrows += 1;
                    state[i] = 1;
                    work.push((i, true));
                    work.push((body as usize, false));
                    (1, body, 0)
                }
                Node::App(f, x) => {
                    variables += 1;
                    arrows += 1;
                    work.push((x as usize, false));
                    work.push((f as usize, false));
                    (2, f, x)
                }
                Node::Base(base @ (1 | 2)) => (3, base, 0),
                _ => return Err(Error::InvalidAst("expected unary STLC".into())),
            };
            nodes[i].write(SourceNode { tag, a, b });
        }

        if visited != n {
            return Err(Error::InvalidAst("unreachable AST nodes".into()));
        }

        if 3 + variables + arrows - 1 > i32::MAX as usize / 12 {
            return Err(Error::Capacity("unary arena exceeds 32-bit frontier limit"));
        }

        // SAFETY: every first visit writes nodes[i]; revisits are rejected and
        // visited == n proves every entry was written. Error paths drop MaybeUninit.
        let nodes = unsafe { nodes.assume_init() }.into_vec();
        Ok(Self {
            nodes,
            variables,
            arrows,
            root: ast.root as usize,
        })
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn root_node(&self) -> u32 {
        self.root as u32
    }

    pub fn type_count(&self) -> usize {
        3 + self.variables + self.arrows
    }

    pub fn equation_count(&self) -> usize {
        self.arrows
    }

    fn problem(&self, retain_equations: bool) -> Problem {
        Problem {
            arena: vec![
                TInfo {
                    arg1: 0,
                    ret: 0
                };
                self.type_count()
            ],
            equations: if retain_equations {
                vec![Equation { left: 0, right: 0 }; self.arrows]
            } else {
                Vec::new()
            },
            arrow_capacity: Some(self.arrows),
            node_types: vec![0; self.nodes.len()],
            binder_types: vec![[0; 2]; self.nodes.len()],
            root: 0,
        }
    }

    /// Linear CPU lowering, with exact allocation and no arrow-interning hashmap.
    /// Every lambda/application owns one arrow; equivalent arrows need not share IDs.
    pub fn compile(&self) -> Problem {
        self.compile_inner(true)
    }

    fn compile_inner(&self, retain_equations: bool) -> Problem {
        // Low 32 bits count variables, high 32 bits count arrows. The size bound
        // prevents low-word overflow, so ordinary integer addition counts both.
        let mut prefix = Vec::with_capacity(self.nodes.len());
        let mut sum = 0u64;

        for node in &self.nodes {
            prefix.push(sum);
            sum += match node.tag {
                1 => (1u64 << 32) + 2,
                2 => (1u64 << 32) + 1,
                _ => 0,
            };
        }

        let first_arrow = (3 + self.variables) as u32;
        let type_of = |i: usize| {
            let node = self.nodes[i];
            match node.tag {
                0 => 4 + prefix[node.a as usize] as u32,
                3 => node.a,
                _ => 3 + prefix[i] as u32,
            }
        };
        let mut p = self.problem(retain_equations);

        for (i, t) in p.arena.iter_mut().enumerate() {
            *t = TInfo {
                arg1: 0,
                ret: i as u32,
            };
        }

        for i in 1..=2 {
            p.arena[i].arg1 = crate::BASE_ARG;
        }

        for (i, node) in self.nodes.iter().enumerate() {
            p.node_types[i] = type_of(i);
            if node.tag == 1 || node.tag == 2 {
                let equation = (prefix[i] >> 32) as usize;
                let tid = first_arrow + equation as u32;
                let (left, arg1, ret) = if node.tag == 1 {
                    let binder = type_of(i) + 1;
                    p.binder_types[i][0] = binder;
                    (type_of(i), binder, type_of(node.a as usize))
                } else {
                    (
                        type_of(node.a as usize),
                        type_of(node.b as usize),
                        type_of(i),
                    )
                };
                p.arena[tid as usize] = TInfo { arg1, ret };

                if retain_equations {
                    p.equations[equation] = Equation { left, right: tid };
                }
            }
        }

        p.root = p.node_types[self.root];
        p
    }
}

#[cfg(test)]
#[path = "unary/preparation_tests.rs"]
mod preparation_tests;

/// The compiled arena, source maps and inferred type representatives.
pub struct Solution {
    pub problem: Problem,
    pub labels: Vec<TypeId>,
}

/// Conventional eager inference returning the arena and its type representatives.
pub fn infer(ast: &ResolvedAst) -> Result<Solution, Error> {
    let problem = ast.compile();
    let labels = crate::sequential::solve(&problem)?;
    Ok(Solution { problem, labels })
}

/// Host-queryable inferred types, without original constraints or variable records.
/// IDs 1/2 are bases; IDs 3..first_arrow are variables. Arrows are indexed by
/// `tid - first_arrow` and contain normalized [argument, result] references.
/// Each source row is [node type, binder type], with zero for an absent binder.
pub struct Typing {
    pub root_node: u32,
    pub first_arrow: TypeId,
    pub arrows: Vec<[TypeId; 2]>,
    pub source: Vec<[TypeId; 2]>,
}

impl Typing {
    /// One type level; children share this graph's ID namespace, never expand trees.
    pub fn get(&self, tid: TypeId) -> Option<TInfo> {
        if tid == 0 {
            return None;
        }

        let (arg1, ret) = if tid >= self.first_arrow {
            let &[a, r] = self.arrows.get((tid - self.first_arrow) as usize)?;
            (a, r)
        } else {
            (if tid <= 2 { crate::BASE_ARG } else { 0 }, tid)
        };

        Some(TInfo { arg1, ret })
    }

    pub fn node(&self, id: u32) -> Option<TInfo> {
        self.get(self.source.get(id as usize)?[0])
    }

    pub fn binder(&self, id: u32) -> Option<TInfo> {
        self.get(self.source.get(id as usize)?[1])
    }
}

/// Conventional eager inference with streamed local equations and whole-tree output.
/// All lowering, unification, occurs checking and result construction happen here.
pub fn infer_types(ast: &ResolvedAst) -> Result<Typing, Error> {
    let p = ast.compile_inner(false);
    let first_arrow = (3 + ast.variables) as u32;
    let mut next_arrow = first_arrow;
    let equations = ast.nodes.iter().enumerate().filter_map(|(i, node)| {
        let left = match node.tag {
            1 => p.node_types[i],
            2 => p.node_types[node.a as usize],
            _ => return None,
        };
        let right = next_arrow;
        next_arrow += 1;
        Some(Equation { left, right })
    });

    let labels = crate::sequential::solve_generated(&p, equations)?;
    let arrows = p.arena[first_arrow as usize..]
        .iter()
        .map(|t| [labels[t.arg1 as usize], labels[t.ret as usize]])
        .collect();

    let mut source = p.binder_types;
    for (i, row) in source.iter_mut().enumerate() {
        *row = [labels[p.node_types[i] as usize], labels[row[0] as usize]];
    }

    Ok(Typing {
        root_node: ast.root as u32,
        first_arrow,
        arrows,
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_rules_and_source_maps() {
        use std::mem::{align_of, offset_of, size_of};
        assert_eq!((size_of::<SourceNode>(), align_of::<SourceNode>()), (12, 4));
        assert_eq!(
            (offset_of!(SourceNode, a), offset_of!(SourceNode, b)),
            (4, 8)
        );
        assert_eq!(
            (size_of::<[TypeId; 2]>(), align_of::<[TypeId; 2]>()),
            (8, 4)
        );
        let ast = Ast {
            nodes: vec![Node::Lam(1), Node::Var(0, 0)],
            root: 0,
        };
        let p = ResolvedAst::new(&ast).unwrap().compile();
        assert_eq!(p.node_types, [3, 4]);
        assert_eq!(p.binder_types, [[4, 0], [0, 0]]);
        assert_eq!(
            p.arena[5],
            TInfo {
                arg1: 4,
                ret: 4
            }
        );
        crate::validate_problem(&p).unwrap();
        assert_eq!(
            crate::readback::depth(&p, &crate::sequential::solve(&p).unwrap()).unwrap(),
            1
        );
    }

    #[test]
    fn malformed_and_nonunary_inputs_are_rejected() {
        for ast in [
            Ast {
                nodes: vec![Node::Var(0, 0)],
                root: 0,
            },
            Ast {
                nodes: vec![Node::Lam(2)],
                root: 0,
            },
            Ast {
                nodes: vec![Node::Lam(0)],
                root: 0,
            },
            Ast {
                nodes: vec![Node::Base(1), Node::Base(2)],
                root: 0,
            },
            Ast {
                nodes: vec![Node::Lam(1), Node::Var(0, 1)],
                root: 0,
            },
            Ast {
                nodes: vec![Node::Base(3)],
                root: 0,
            },
        ] {
            assert!(ResolvedAst::new(&ast).is_err());
        }
    }

    #[test]
    fn reversed_storage_keeps_resolved_scope_and_source_types() {
        let source = ResolvedAst::new(&Ast {
            nodes: vec![Node::Var(1, 0), Node::Lam(0)],
            root: 1,
        })
        .unwrap();
        let result = infer_types(&source).unwrap();
        assert_eq!(result.root_node, 1);
        let arrow = result.node(1).unwrap();
        assert_eq!(arrow.arg1, arrow.ret);
        assert_eq!(result.source[0][0], arrow.arg1);
        assert_eq!(result.source[1][1], arrow.arg1);
    }
}
