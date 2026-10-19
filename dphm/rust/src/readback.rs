// Generated/maintained with AI; see repository disclaimers.md.
//! Readback is a view of a shared graph, not an expanded tree.
use crate::{Error, Problem, TypeId};

// Check solution shape, not principality (which is the solver's obligation).
pub(crate) fn validate_labels(p: &Problem, labels: &[TypeId]) -> Result<(), Error> {
    crate::validate_problem(p)?;

    if labels.len() != p.arena.len() || labels.first() != Some(&0) {
        return Err(Error::InvalidProblem("wrong label dimensions"));
    }

    for (i, &r) in labels.iter().enumerate().skip(1) {
        if r == 0 || r as usize >= labels.len() || labels[r as usize] != r {
            return Err(Error::InvalidProblem("labels are not flat representatives"));
        }

        let t = p.arena[i];
        let root = p.arena[r as usize];
        if (t.arg1 == crate::BASE_ARG && r != i as TypeId) || (t.is_arrow() && !root.is_arrow()) {
            return Err(Error::InvalidProblem("representative loses rigid shape"));
        }

        if t.is_arrow() && t.signature(labels) != root.signature(labels) {
            return Err(Error::InvalidProblem(
                "constructor class is not downward closed",
            ));
        }
    }

    if p.equations
        .iter()
        .any(|e| labels[e.left as usize] != labels[e.right as usize])
    {
        return Err(Error::InvalidProblem("unsatisfied input equation"));
    }

    Ok(())
}

/// Leaves have depth zero; each compound adds one to its deepest child.
pub fn depth(p: &Problem, labels: &[TypeId]) -> Result<u32, Error> {
    validate_labels(p, labels)?;
    depth_validated(p, labels)
}

// Call only after validate_labels; the traversal itself still detects cycles.
pub(crate) fn depth_validated(p: &Problem, labels: &[TypeId]) -> Result<u32, Error> {
    let mut state = vec![0u8; labels.len()];
    let mut heights = vec![0u32; labels.len()];
    let mut work = Vec::new();
    let mut maximum = 0;

    for &start in labels.iter().skip(1) {
        if state[start as usize] == 2 {
            continue;
        }

        work.push((start, false));

        while let Some((id, leaving)) = work.pop() {
            let i = id as usize;
            let t = p.arena[i];
            if state[i] == 2 {
                continue;
            }

            if leaving {
                heights[i] = 1 + t
                    .children()
                    .into_iter()
                    .map(|c| heights[labels[c as usize] as usize])
                    .max()
                    .unwrap();
                maximum = maximum.max(heights[i]);
                state[i] = 2;
            } else {
                if state[i] == 1 {
                    return Err(Error::InfiniteType);
                }

                if !t.is_arrow() {
                    state[i] = 2;
                    continue;
                }

                state[i] = 1;
                work.push((id, true));
                for child in t.children().into_iter().rev().filter(|&c| c != 0) {
                    work.push((labels[child as usize], false));
                }
            }
        }
    }

    Ok(maximum)
}
