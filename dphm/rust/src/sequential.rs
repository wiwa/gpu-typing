// Generated/maintained with AI; see repository disclaimers.md.
//! Sequential reference solver: eager structural unification, then an occurs-check.

use crate::{Equation, Error, Problem, TypeId};

fn find(parent: &mut [u32], mut id: usize) -> usize {
    let mut root = id;
    while parent[root] as usize != root {
        root = parent[root] as usize;
    }

    while parent[id] as usize != id {
        let next = parent[id] as usize;
        parent[id] = root as u32;
        id = next;
    }

    root
}

/// Return one type-bearing representative for every original type ID.
pub fn solve(problem: &Problem) -> Result<Vec<TypeId>, Error> {
    crate::validate_problem(problem)?;
    solve_generated(problem, problem.equations.iter().copied())
}

// Internal generated-input path: the producer guarantees valid arena/child IDs
// and equations. It may stream equations instead of retaining an input batch.
pub(crate) fn solve_generated(
    problem: &Problem,
    equations: impl IntoIterator<Item = Equation>,
) -> Result<Vec<TypeId>, Error> {
    let n = problem.arena.len();

    // Validated arenas fit in 32-bit IDs; avoid three machine-word arrays.
    let mut parent: Vec<u32> = (0..n as u32).collect();
    let mut size = vec![1u32; n];
    // Union-by-size chooses the root; `shape[root]` remembers its rigid member,
    // or a variable member when the class has no known constructor.
    let mut shape: Vec<u32> = (0..n as u32).collect();
    let mut pending = Vec::new();

    for equation in equations {
        pending.push((equation.left, equation.right));

        while let Some((left, right)) = pending.pop() {
            let mut a = find(&mut parent, left as usize);
            let mut b = find(&mut parent, right as usize);
            if a == b {
                continue;
            }

            let x = problem.arena[shape[a] as usize];
            let y = problem.arena[shape[b] as usize];
            let chosen = if x.arg1 == 0 { shape[b] } else { shape[a] };

            if x.arg1 != 0 && y.arg1 != 0 {
                if x.is_base() || y.is_base() {
                    if shape[a] != shape[b] {
                        return Err(Error::RigidConflict);
                    }
                } else {
                    pending.push((x.arg1, y.arg1));
                    pending.push((x.ret, y.ret));
                }
            }

            if size[a] < size[b] {
                std::mem::swap(&mut a, &mut b);
            }
            parent[b] = a as u32;
            size[a] += size[b];
            shape[a] = chosen;
        }
    }

    let mut labels = Vec::with_capacity(n);
    for id in 0..n {
        labels.push(shape[find(&mut parent, id)] as TypeId);
    }

    // Structural merging may create a cycle; even an equation-free input may
    // already contain one. Check the entire arena, not only the source root.
    crate::readback::validate_labels(problem, &labels)?;
    crate::readback::depth_validated(problem, &labels)?;

    Ok(labels)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Equation, TInfo};

    fn problem(extra: &[(TypeId, TypeId)], equations: &[(TypeId, TypeId)]) -> Problem {
        let mut arena = vec![
            TInfo { arg1: 0, ret: 0 },
            TInfo {
                arg1: crate::BASE_ARG,
                ret: 1,
            },
            TInfo {
                arg1: crate::BASE_ARG,
                ret: 2,
            },
        ];
        for &(arg1, ret) in extra {
            arena.push(TInfo { arg1, ret });
        }
        Problem {
            arena,
            equations: equations
                .iter()
                .map(|&(left, right)| Equation { left, right })
                .collect(),
            node_types: vec![1],
            arrow_capacity: None,
            binder_types: vec![],
            root: 1,
        }
    }

    #[test]
    fn variables_can_acquire_base_types() {
        let p = problem(&[(0, 3), (0, 4)], &[(3, 4), (4, 2)]);
        let labels = solve(&p).unwrap();
        assert_eq!(labels, vec![0, 1, 2, 2, 2]);
    }

    #[test]
    fn distinct_bases_and_base_arrow_conflict() {
        for equations in [&[(1, 2)][..], &[(1, 3)][..]] {
            assert!(matches!(
                solve(&problem(&[(1, 2)], equations)),
                Err(Error::RigidConflict)
            ));
        }
    }

    #[test]
    fn indirectly_equal_arrows_unify_their_children() {
        let p = problem(&[(0, 3), (3, 2), (1, 2), (0, 6)], &[(4, 6), (6, 5)]);
        let labels = solve(&p).unwrap();
        assert_eq!(labels[3], 1);
        assert_eq!(labels[4], labels[5]);
        assert_eq!(labels[5], labels[6]);
        assert!(p.arena[labels[6] as usize].is_arrow());
        for &label in &labels {
            assert_eq!(labels[label as usize], label);
        }
    }

    #[test]
    fn cycles_are_rejected_even_without_equations() {
        assert!(matches!(
            solve(&problem(&[(3, 1)], &[])),
            Err(Error::InfiniteType)
        ));
        assert!(matches!(
            solve(&problem(&[(0, 3), (3, 1)], &[(3, 4)])),
            Err(Error::InfiniteType)
        ));
    }

    #[test]
    fn deeply_nested_arrows_do_not_use_the_call_stack() {
        let mut p = problem(&[], &[]);
        let (mut left, mut right) = (1, 1);
        for _ in 0..10_000 {
            let a = p.arena.len() as TypeId;
            p.arena.push(TInfo { arg1: 2, ret: left });
            p.arena.push(TInfo {
                arg1: 2,
                ret: right,
            });
            (left, right) = (a, a + 1);
        }
        p.equations.push(Equation { left, right });
        let labels = solve(&p).unwrap();
        assert_eq!(labels[left as usize], labels[right as usize]);
    }
}
