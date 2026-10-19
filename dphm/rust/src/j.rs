// Generated/maintained with AI; see repository disclaimers.md.
//! Algorithm J specialized to monomorphic STLC.
//!
//! Mutable type links carry substitutions; applications unify immediately.
//! Type-variable bindings perform occurs checks. There are no bulk-solving rounds.

use crate::{
    unary::{ResolvedAst, Typing},
    Error, TypeId,
};

#[derive(Clone, Copy)]
enum Ty {
    Var,
    Base(u32),
    Arrow(TypeId, TypeId),
    Link(TypeId),
}

// Explicit continuations for recursive unification: join arrows after their children.
enum Request {
    Unify(TypeId, TypeId),
    Join(TypeId, TypeId),
}

struct Types {
    rows: Vec<Ty>,
    sizes: Vec<u32>,
    seen: Vec<u32>,
    epoch: u32,
    occurs_work: Vec<TypeId>,
    pending: Vec<Request>,
}

impl Types {
    fn new(capacity: usize) -> Self {
        let mut s = Self {
            rows: Vec::with_capacity(capacity),
            sizes: Vec::with_capacity(capacity),
            seen: Vec::with_capacity(capacity),
            epoch: 0,
            occurs_work: Vec::new(),
            pending: Vec::new(),
        };
        s.push(Ty::Var); // Absent-binder sentinel.
        s.push(Ty::Base(1));
        s.push(Ty::Base(2));
        s
    }

    fn push(&mut self, ty: Ty) -> TypeId {
        let id = self.rows.len() as TypeId;
        self.rows.push(ty);
        self.sizes.push(1);
        self.seen.push(0);
        id
    }

    fn find(&mut self, id: TypeId) -> TypeId {
        let mut root = id;
        while let Ty::Link(next) = self.rows[root as usize] {
            root = next;
        }

        let mut id = id;
        while let Ty::Link(next) = self.rows[id as usize] {
            self.rows[id as usize] = Ty::Link(root);
            id = next;
        }
        root
    }

    // Called only for compatible roots, after arrow children have unified.
    fn join(&mut self, mut a: TypeId, mut b: TypeId) {
        let value = if matches!(self.rows[a as usize], Ty::Var) {
            self.rows[b as usize]
        } else {
            self.rows[a as usize]
        };
        if self.sizes[a as usize] < self.sizes[b as usize] {
            std::mem::swap(&mut a, &mut b);
        }
        self.sizes[a as usize] += self.sizes[b as usize];
        self.rows[a as usize] = value;
        self.rows[b as usize] = Ty::Link(a);
    }

    fn occurs(&mut self, variable: TypeId, ty: TypeId) -> bool {
        if !matches!(self.rows[ty as usize], Ty::Arrow(..)) {
            return false;
        }

        // At most one check per variable binding; checked ASTs fit u31.
        self.epoch += 1;
        self.occurs_work.clear();
        self.occurs_work.push(ty);

        while let Some(id) = self.occurs_work.pop() {
            let id = self.find(id);
            if id == variable {
                return true;
            }

            if self.seen[id as usize] == self.epoch {
                continue;
            }
            self.seen[id as usize] = self.epoch;
            if let Ty::Arrow(arg, ret) = self.rows[id as usize] {
                self.occurs_work.extend([arg, ret]);
            }
        }

        false
    }

    fn unify(&mut self, a: TypeId, b: TypeId) -> Result<(), Error> {
        self.pending.push(Request::Unify(a, b));

        while let Some(request) = self.pending.pop() {
            let (a, b, finish) = match request {
                Request::Unify(a, b) => (a, b, false),
                Request::Join(a, b) => (a, b, true),
            };
            let (a, b) = (self.find(a), self.find(b));

            if a == b {
                continue;
            }

            if finish {
                self.join(a, b);
                continue;
            }

            match (self.rows[a as usize], self.rows[b as usize]) {
                (Ty::Var, _) => {
                    if self.occurs(a, b) {
                        return Err(Error::InfiniteType);
                    }
                    self.join(a, b);
                }
                (_, Ty::Var) => {
                    if self.occurs(b, a) {
                        return Err(Error::InfiniteType);
                    }
                    self.join(a, b);
                }
                (Ty::Base(x), Ty::Base(y)) if x == y => self.join(a, b),
                (Ty::Arrow(x, y), Ty::Arrow(z, w)) => {
                    // Cache completed structural equality, not a global Up pass.
                    // Joining after both children keeps the type graph acyclic.
                    self.pending.extend([
                        Request::Join(a, b),
                        Request::Unify(y, w),
                        Request::Unify(x, z),
                    ]);
                }
                _ => return Err(Error::RigidConflict),
            }
        }

        Ok(())
    }
}

/// Infer every source/binder type with the ordinary monomorphic J rules:
///
/// ```text
/// infer(x, env)       = env[x]
/// infer(\x.body, env) = let a = fresh()
///                      let b = infer(body, env[x := a])
///                      return Arrow(a, b)
/// infer(f x, env)     = let tf = infer(f, env), tx = infer(x, env)
///                      let r = fresh()
///                      unify(tf, Arrow(tx, r)); return r
/// ```
///
/// Binder references are resolved before inference: `source[binder][1]` is the
/// environment lookup. Each `source[i]` stores [expression type, parameter type];
/// the second entry is used only for lambdas. The enter/leave stack implements
/// recursive syntax traversal without relying on the machine call stack.
pub fn infer_types(ast: &ResolvedAst) -> Result<Typing, Error> {
    let mut types = Types::new(3 + 2 * ast.arrows);
    let mut source = vec![[0; 2]; ast.nodes.len()];
    let mut arrows = Vec::with_capacity(ast.arrows);
    let mut work = vec![(ast.root, false)];

    while let Some((i, leaving)) = work.pop() {
        let node = ast.nodes[i];
        if !leaving {
            match node.tag {
                // Variable: look up its resolved binder's parameter type.
                0 => source[i][0] = source[node.a as usize][1],

                // Constant: its reserved Bool/Int type is already known.
                3 => source[i][0] = node.a,

                // Lambda: allocate the parameter type before inferring its body.
                1 => {
                    source[i][1] = types.push(Ty::Var);
                    work.extend([(i, true), (node.a as usize, false)]);
                }

                // Application: infer function, then argument, then apply its rule.
                2 => work.extend([
                    (i, true),
                    (node.b as usize, false),
                    (node.a as usize, false),
                ]),

                _ => unreachable!("ResolvedAst validates syntax"),
            }
        } else {
            // Lambda returns parameter -> body; application expects argument -> r.
            let (arg, ret) = if node.tag == 1 {
                (source[i][1], source[node.a as usize][0])
            } else {
                (source[node.b as usize][0], types.push(Ty::Var))
            };

            let arrow = types.push(Ty::Arrow(arg, ret));
            arrows.push(arrow);

            if node.tag == 2 {
                types.unify(source[node.a as usize][0], arrow)?;
            }

            source[i][0] = if node.tag == 1 { arrow } else { ret };
        }
    }

    // Inference is complete. Convert IDs to the shared query format, without
    // solving further constraints or expanding the type DAG into a tree.
    // Keep one result row per allocated arrow.
    let roots: Vec<_> = (0..types.rows.len() as TypeId)
        .map(|i| types.find(i))
        .collect();

    let mut names = vec![0; roots.len()];
    let mut variable = 3;

    for i in 1..roots.len() {
        if roots[i] as usize != i {
            continue;
        }
        match types.rows[i] {
            Ty::Var => {
                names[i] = variable;
                variable += 1;
            }
            Ty::Base(base) => names[i] = base,
            _ => (),
        }
    }

    let first_arrow = (3 + ast.arrows) as TypeId; // One variable per lambda/application.
    for (i, &id) in arrows.iter().enumerate() {
        let name = &mut names[roots[id as usize] as usize];
        if *name == 0 {
            *name = first_arrow + i as TypeId;
        }
    }

    let normalize = |id: TypeId| names[roots[id as usize] as usize];
    let arrows = arrows
        .into_iter()
        .map(|id| {
            let Ty::Arrow(a, b) = types.rows[roots[id as usize] as usize] else {
                unreachable!()
            };
            [normalize(a), normalize(b)]
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
    fn completed_arrow_pairs_preserve_dag_sharing() {
        let mut types = Types::new(100);
        let mut a = types.push(Ty::Var);
        let mut b = types.push(Ty::Var);

        for _ in 0..40 {
            a = types.push(Ty::Arrow(a, a));
            b = types.push(Ty::Arrow(b, b));
        }

        types.unify(a, b).unwrap();
        assert_eq!(types.find(a), types.find(b));
    }
}
