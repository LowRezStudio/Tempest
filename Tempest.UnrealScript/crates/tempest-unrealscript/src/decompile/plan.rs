//! Minimal `dependson(...)` sets for a decompiled package.
//!
//! The compiler parses classes in class-tree order (parents first, siblings by
//! name), pulling a `DependsOn` class and its subclasses forward. A class that uses
//! a struct/enum declared in an unrelated same-package class must be parsed after
//! it. Starting from no dependencies, this simulates the parse order and adds a
//! dependency wherever a referenced owner would still be parsed later, until the
//! order is stable; dependencies that would create a cycle are left out.

use std::collections::{BTreeSet, HashMap, HashSet};

use crate::context::{Context, ObjRef};

pub struct Plan {
    /// Class-tree parse order of the local classes.
    pub order: Vec<ObjRef>,
    /// Chosen `dependson` classes per class.
    pub depends: HashMap<ObjRef, Vec<ObjRef>>,
}

fn tree_order(ctx: &Context, classes: &[ObjRef]) -> Vec<ObjRef> {
    let mut children: HashMap<Option<ObjRef>, Vec<ObjRef>> = HashMap::new();
    let mut seen: HashSet<ObjRef> = HashSet::new();
    for &c in classes {
        let mut cur = c;
        loop {
            if !seen.insert(cur) {
                break;
            }
            let sup = ctx.super_of(cur);
            children.entry(sup).or_default().push(cur);
            match sup {
                Some(s) => cur = s,
                None => break,
            }
        }
    }
    for v in children.values_mut() {
        v.sort_by_key(|r| ctx.name(*r).to_ascii_lowercase());
    }
    let local: HashSet<ObjRef> = classes.iter().copied().collect();
    let mut out = Vec::new();
    let mut stack: Vec<ObjRef> = children.get(&None).cloned().unwrap_or_default().into_iter().rev().collect();
    while let Some(r) = stack.pop() {
        if local.contains(&r) {
            out.push(r);
        }
        if let Some(ch) = children.get(&Some(r)) {
            stack.extend(ch.iter().rev().copied());
        }
    }
    out
}

struct Sim<'a> {
    ctx: &'a Context,
    #[allow(dead_code)]
    order: &'a [ObjRef],
    local: &'a HashSet<ObjRef>,
    subtree: &'a HashMap<ObjRef, Vec<ObjRef>>,
    deps: &'a HashMap<ObjRef, Vec<ObjRef>>,
    parsed: HashSet<ObjRef>,
    stack: Vec<ObjRef>,
    out: Vec<ObjRef>,
}

impl<'a> Sim<'a> {
    /// Mirrors the compiler driver's `parse_class` (`ParseScripts`): re-entrant
    /// while a class handles its dependencies; a dependency already parsed is
    /// skipped together with its subclasses.
    fn parse(&mut self, c: ObjRef) -> Result<(), ()> {
        if self.parsed.contains(&c) {
            return Ok(());
        }
        if self.stack.len() > 4096 {
            return Err(());
        }
        self.stack.push(c);
        if let Some(s) = self.ctx.super_of(c) {
            if self.local.contains(&s) {
                self.parse(s)?;
            }
        }
        if let Some(ds) = self.deps.get(&c).cloned() {
            for d in ds {
                if self.parsed.contains(&d) {
                    continue;
                }
                if self.depends_transitively(d, c) {
                    return Err(());
                }
                self.parse(d)?;
                if let Some(sub) = self.subtree.get(&d).cloned() {
                    for x in sub {
                        self.parse(x)?;
                    }
                }
            }
        }
        self.stack.pop();
        if self.parsed.contains(&c) {
            return Ok(());
        }
        self.parsed.insert(c);
        self.out.push(c);
        Ok(())
    }

    /// `IsDependentOn`: does `class`'s dependson list reach `target`?
    fn depends_transitively(&self, class: ObjRef, target: ObjRef) -> bool {
        let mut seen: HashSet<ObjRef> = HashSet::new();
        let mut todo = vec![class];
        while let Some(x) = todo.pop() {
            if !seen.insert(x) {
                continue;
            }
            if let Some(ds) = self.deps.get(&x) {
                for &d in ds {
                    if d == target {
                        return true;
                    }
                    todo.push(d);
                }
            }
        }
        false
    }
}

fn simulate(ctx: &Context, order: &[ObjRef], local: &HashSet<ObjRef>, subtree: &HashMap<ObjRef, Vec<ObjRef>>, deps: &HashMap<ObjRef, Vec<ObjRef>>) -> Option<Vec<ObjRef>> {
    let mut sim = Sim { ctx, order, local, subtree, deps, parsed: HashSet::new(), stack: Vec::new(), out: Vec::new() };
    for &c in order {
        sim.parse(c).ok()?;
    }
    Some(sim.out)
}

/// `refs`: for each class, the same-package non-ancestor classes whose types it uses.
pub fn plan_dependson(ctx: &Context, classes: &[ObjRef], refs: &HashMap<ObjRef, BTreeSet<ObjRef>>) -> Plan {
    let order = tree_order(ctx, classes);
    let local: HashSet<ObjRef> = classes.iter().copied().collect();
    let mut subtree: HashMap<ObjRef, Vec<ObjRef>> = HashMap::new();
    for &c in &order {
        for &x in &order {
            if x != c && ctx.derives_from(x, c) {
                subtree.entry(c).or_default().push(x);
            }
        }
    }
    let mut deps: HashMap<ObjRef, Vec<ObjRef>> = HashMap::new();
    let mut rejected: HashSet<(ObjRef, ObjRef)> = HashSet::new();
    let mut guard = 0;
    'outer: loop {
        guard += 1;
        if guard > 5000 {
            break;
        }
        let Some(current) = simulate(ctx, &order, &local, &subtree, &deps) else { break };
        let pos: HashMap<ObjRef, usize> = current.iter().enumerate().map(|(i, &r)| (r, i)).collect();
        for &c in &current {
            let Some(rs) = refs.get(&c) else { continue };
            for &o in rs {
                if !local.contains(&o) || o == c || rejected.contains(&(c, o)) {
                    continue;
                }
                if deps.get(&c).map(|v| v.contains(&o)).unwrap_or(false) {
                    continue;
                }
                let (Some(&pc), Some(&po)) = (pos.get(&c), pos.get(&o)) else { continue };
                if po < pc {
                    continue;
                }
                deps.entry(c).or_default().push(o);
                if simulate(ctx, &order, &local, &subtree, &deps).is_none() {
                    deps.get_mut(&c).unwrap().pop();
                    rejected.insert((c, o));
                    continue;
                }
                continue 'outer;
            }
        }
        break;
    }
    Plan { order, depends: deps }
}
