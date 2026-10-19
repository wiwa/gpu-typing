// Generated/maintained with AI; see repository disclaimers.md.
//! Optimized J-style monomorphic inference, not the direct textbook presentation.
//!
//! Compared with `j.rs`, known function heads reuse their argument/result types,
//! records are packed, and occurs checking runs once at the end. Inference stays
//! syntax-directed and eager; there are no DPHM rounds or global Up passes.

use crate::{
    unary::{ResolvedAst, Typing},
    Error, TypeId,
};

// Private mutable types, not the GPU arena: arg=0 is a variable/link, MAX a base,
// otherwise an arrow. A variable points to itself; a link points to another row.
#[derive(Clone, Copy)]
struct Ty {
    arg: TypeId,
    ret: TypeId,
}

struct Types {
    rows: Vec<Ty>,
    ranks: Vec<u8>,
    pending: Vec<(TypeId, TypeId)>,
}

impl Types {
    fn new(capacity: usize) -> Self {
        let mut s = Self {
            rows: Vec::with_capacity(capacity),
            ranks: Vec::with_capacity(capacity),
            pending: Vec::new(),
        };
        s.variable(); // Absent-binder sentinel.
        for base in 1..=2 {
            s.push(Ty {
                arg: u32::MAX,
                ret: base,
            });
        }
        s
    }

    fn push(&mut self, ty: Ty) -> TypeId {
        let id = self.rows.len() as TypeId;
        self.rows.push(ty);
        self.ranks.push(0);
        id
    }

    fn variable(&mut self) -> TypeId {
        self.push(Ty {
            arg: 0,
            ret: self.rows.len() as TypeId,
        })
    }

    fn arrow(&mut self, arg: TypeId, ret: TypeId) -> TypeId {
        self.push(Ty { arg, ret })
    }

    // One parent hop; after finish() every parent is also the representative.
    fn parent(&self, id: TypeId) -> TypeId {
        let row = self.rows[id as usize];
        if row.arg == 0 {
            row.ret
        } else {
            id
        }
    }

    fn find(&mut self, id: TypeId) -> TypeId {
        let mut root = id;
        while self.parent(root) != root {
            root = self.parent(root);
        }

        let mut id = id;
        while self.parent(id) != id {
            let next = self.parent(id);
            self.rows[id as usize].ret = root;
            id = next;
        }
        root
    }

    fn unify(&mut self, a: TypeId, b: TypeId) -> Result<(), Error> {
        self.pending.push((a, b));

        while let Some((a, b)) = self.pending.pop() {
            let (mut a, mut b) = (self.find(a), self.find(b));
            if a == b {
                continue;
            }

            let (x, y) = (self.rows[a as usize], self.rows[b as usize]);
            if x.arg != 0 && y.arg != 0 {
                if x.arg == u32::MAX || y.arg == u32::MAX {
                    return Err(Error::RigidConflict); // Distinct rigid roots.
                }
                self.pending.extend([(x.ret, y.ret), (x.arg, y.arg)]);
            }

            let shape = if x.arg == 0 { y } else { x };
            // Keep base IDs fixed. Other unions use rank, moving the known head
            // into the winning row. Linking first also terminates cyclic requests.
            if b <= 2 || (a > 2 && self.ranks[a as usize] < self.ranks[b as usize]) {
                std::mem::swap(&mut a, &mut b);
            }
            if self.ranks[a as usize] == self.ranks[b as usize] {
                self.ranks[a as usize] += 1;
            }
            self.rows[a as usize] = shape;
            if shape.arg == 0 {
                self.rows[a as usize].ret = a;
            }
            self.rows[b as usize] = Ty { arg: 0, ret: a };
        }

        Ok(())
    }

    fn finish(&mut self) -> Result<(), Error> {
        for i in 0..self.rows.len() as TypeId {
            self.find(i);
        }

        // All links are now flat. Check every component, including unused source
        // subterms, once: grey means on the current path, black means completed.
        let mut color = vec![0u8; self.rows.len()];
        let mut work = Vec::new();

        for i in 3..self.rows.len() as TypeId {
            work.push((i, false));
            while let Some((id, leaving)) = work.pop() {
                let id = self.parent(id);
                let row = self.rows[id as usize];
                if row.arg == 0 || row.arg == u32::MAX {
                    continue;
                }
                let state = &mut color[id as usize];
                if leaving {
                    *state = 2;
                    continue;
                }
                match *state {
                    1 => return Err(Error::InfiniteType),
                    2 => continue,
                    _ => (),
                }
                *state = 1;
                work.extend([(id, true), (row.ret, false), (row.arg, false)]);
            }
        }

        Ok(())
    }
}

/// Infer all source/binder types using ordinary eager mutable-type unification.
/// Known arrows need no fresh application type; unknown functions acquire one.
/// Cycles are rejected once after inference, before any result is returned.
pub fn infer_types(ast: &ResolvedAst) -> Result<Typing, Error> {
    let mut types = Types::new(3 + 2 * ast.arrows);
    let mut source = vec![[0; 2]; ast.nodes.len()];
    let mut arrows = Vec::with_capacity(ast.arrows);
    let mut work = vec![(ast.root, false)];

    while let Some((i, leaving)) = work.pop() {
        let node = ast.nodes[i];
        if !leaving {
            match node.tag {
                0 => source[i][0] = source[node.a as usize][1],
                3 => source[i][0] = node.a,

                1 => {
                    source[i][1] = types.variable();
                    work.extend([(i, true), (node.a as usize, false)]);
                }

                2 => work.extend([
                    (i, true),
                    (node.b as usize, false),
                    (node.a as usize, false),
                ]),

                _ => unreachable!("ResolvedAst validates syntax"),
            }
        } else if node.tag == 1 {
            let arrow = types.arrow(source[i][1], source[node.a as usize][0]);
            arrows.push(arrow);
            source[i][0] = arrow;
        } else {
            let f = types.find(source[node.a as usize][0]);
            let arg = source[node.b as usize][0];
            let head = types.rows[f as usize];

            let (arrow, result) = match head.arg {
                0 => {
                    let result = types.variable();
                    let arrow = types.arrow(arg, result);
                    types.unify(f, arrow)?;
                    (arrow, result)
                }

                u32::MAX => return Err(Error::RigidConflict),

                _ => {
                    types.unify(head.arg, arg)?;
                    (f, head.ret)
                }
            };

            arrows.push(arrow);
            source[i][0] = result;
        }
    }

    types.finish()?;

    // Convert IDs to the shared query format. Keep one arrow observation per
    // lambda/application, including repeated observations of a reused arrow.
    let first_arrow = (3 + ast.arrows) as TypeId;
    let mut names = vec![0; types.rows.len()];
    names[1] = 1;
    names[2] = 2;
    let mut variable = 3;

    for i in 3..types.rows.len() as TypeId {
        if types.parent(i) == i && types.rows[i as usize].arg == 0 {
            names[i as usize] = variable;
            variable += 1;
        }
    }

    for (i, &arrow) in arrows.iter().enumerate() {
        let name = &mut names[types.parent(arrow) as usize];
        if *name == 0 {
            *name = first_arrow + i as TypeId;
        }
    }

    let normalize = |id| names[types.parent(id) as usize];
    let arrows = arrows
        .into_iter()
        .map(|id| {
            let row = types.rows[types.parent(id) as usize];
            [normalize(row.arg), normalize(row.ret)]
        })
        .collect();

    for row in &mut source {
        *row = [normalize(row[0]), normalize(row[1])];
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
    fn shared_dags_and_cycles_terminate() {
        assert_eq!(std::mem::size_of::<Ty>(), 8);
        let mut t = Types::new(100);
        let (mut a, mut b) = (t.variable(), t.variable());

        for _ in 0..40 {
            a = t.arrow(a, a);
            b = t.arrow(b, b);
        }

        t.unify(a, b).unwrap();
        t.finish().unwrap();
        assert_eq!(t.find(a), t.find(b));

        let x = t.variable();
        let f = t.arrow(x, 1);
        t.unify(x, f).unwrap();
        let y = t.variable();
        let g = t.arrow(y, 1);
        t.unify(y, g).unwrap();
        t.unify(f, g).unwrap(); // Must not loop on cyclic intermediate graphs.
        assert_eq!(t.finish(), Err(Error::InfiniteType));
    }
}
