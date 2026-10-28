//! Round-trip checks of the bytecode layers over real scripts (`upk as2 verify`).

use std::fmt;

use super::action::Script;
use super::pcode;

#[derive(Default)]
pub struct Stats {
    pub scripts: usize,
    pub bytes: usize,
    pub decode_failed: usize,
    pub encode_diff: usize,
    pub pcode_diff: usize,
    /// Scripts with something kept as P-code, and the reasons by count.
    pub not_decompiled: usize,
    /// Decompiled source that does not compile, or compiles to something that decompiles differently.
    pub compile_failed: usize,
    pub recompile_diff: usize,
    /// Recompiled bytecode identical to the original (byte for byte / after normalising pools and pushes).
    pub identical: usize,
    pub same_ops: usize,
    pub reasons: std::collections::BTreeMap<String, usize>,
}

impl Stats {
    /// Check one action stream; returns what went wrong.
    pub fn check(&mut self, bytes: &[u8]) -> Vec<String> {
        let orig = bytes;
        let mut out = Vec::new();
        self.scripts += 1;
        self.bytes += bytes.len();
        let s = match Script::decode(bytes) {
            Ok(s) => s,
            Err(e) => {
                self.decode_failed += 1;
                return vec![format!("decode: {e}")];
            }
        };
        match s.encode() {
            Ok(b) if b == bytes => {}
            Ok(b) => {
                self.encode_diff += 1;
                out.push(format!("encode differs ({} vs {} bytes, first at {})", b.len(), bytes.len(), first_diff(&b, bytes)));
            }
            Err(e) => {
                self.encode_diff += 1;
                out.push(format!("encode: {e}"));
            }
        }
        let text = pcode::print(&s);
        match pcode::parse(&text).and_then(|p| p.encode()) {
            Ok(b) if b == bytes => {}
            Ok(b) => {
                self.pcode_diff += 1;
                out.push(format!("P-code round trip differs ({} vs {} bytes, first at {})", b.len(), bytes.len(), first_diff(&b, bytes)));
            }
            Err(e) => {
                self.pcode_diff += 1;
                out.push(format!("P-code: {e}"));
            }
        }
        let problems = super::decompile::problems(&s);
        if !problems.is_empty() {
            self.not_decompiled += 1;
            for p in &problems {
                let key = p.split_once(": ").map(|(_, r)| r).unwrap_or(p);
                let key: String = key.chars().map(|c| if c.is_ascii_digit() { 'N' } else { c }).collect();
                *self.reasons.entry(key).or_default() += 1;
            }
            out.extend(problems.into_iter().map(|p| format!("decompile: {p}")));
        }
        // Decompile, compile, decompile again: the source must come back the same.
        let src = super::decompile::source(&s);
        match super::compile::compile(&src).and_then(|c| c.encode().map(|b| (c, b))) {
            Err(e) => {
                self.compile_failed += 1;
                out.push(format!("compile: {e}"));
            }
            Ok((c, bytes)) => match Script::decode(&bytes) {
                Ok(back) if back == c => {
                    if bytes == orig {
                        self.identical += 1;
                    } else if normal(&s.actions, &[]) == normal(&back.actions, &[]) {
                        self.same_ops += 1;
                    } else if std::env::var_os("AS2_OPDIFF").is_some() {
                        let (a, b) = (normal(&s.actions, &[]), normal(&back.actions, &[]));
                        let i = a.iter().zip(&b).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()));
                        out.push(format!("ops differ at {i}: {:?} vs {:?}", &a[i.saturating_sub(3)..(i + 4).min(a.len())], &b[i.saturating_sub(3)..(i + 4).min(b.len())]));
                    }
                    let again = super::decompile::source(&back);
                    if again != src {
                        self.recompile_diff += 1;
                        let (a, b) = first_line_diff(&src, &again);
                        out.push(format!("recompiled source differs:\n    - {a}\n    + {b}"));
                    }
                }
                Ok(_) => {
                    self.compile_failed += 1;
                    out.push("compiled bytecode does not decode to itself".into());
                }
                Err(e) => {
                    self.compile_failed += 1;
                    out.push(format!("compiled bytecode: {e}"));
                }
            },
        }
        out
    }
}

/// The actions as text with constant references resolved and pushes split,
/// branches as relative distances, nested functions inline: two scripts that
/// differ only in pool order and push grouping compare equal.
fn normal(actions: &[super::action::Action], pool: &[String]) -> Vec<String> {
    use super::action::{Action, Value};
    let mut pool = pool.to_vec();
    let mut out = Vec::new();
    let mut items: Vec<Vec<String>> = Vec::new();
    for a in actions {
        let mut v = Vec::new();
        match a {
            Action::ConstantPool(p) => pool = p.clone(),
            Action::Push(vals) => {
                for x in vals {
                    v.push(match x {
                        Value::Const(i, _) => format!("push {:?}", pool.get(*i as usize)),
                        Value::Str(s) => format!("push {:?}", Some(s)),
                        Value::Int(i) => format!("push num {i}"),
                        Value::Double(d) => format!("push num {d}"),
                        Value::Float(f) => format!("push num {f}"),
                        x => format!("push {x:?}"),
                    });
                }
            }
            Action::Function(f) => {
                v.push(format!("function {:?} {:?} {:?}", f.name, f.v2, f.params));
                v.extend(normal(&f.body, &pool));
                v.push("end function".into());
            }
            a => v.push(format!("{a:?}")),
        }
        items.push(v);
    }
    // Branch targets as labels, in order, so removing an item does not move them.
    let mut targets: Vec<usize> = actions.iter().filter_map(|a| if let Action::Jump(t) | Action::If(t) = a { Some(*t) } else { None }).collect();
    targets.sort_unstable();
    targets.dedup();
    let name = |t: usize| format!("L{}", targets.binary_search(&t).unwrap());
    for (i, (a, v)) in actions.iter().zip(items).enumerate() {
        if targets.binary_search(&i).is_ok() {
            out.push(format!("{}:", name(i)));
        }
        match a {
            Action::Jump(t) => out.push(format!("Jump {}", name(*t))),
            Action::If(t) => out.push(format!("If {}", name(*t))),
            _ => out.extend(v),
        }
    }
    if targets.binary_search(&actions.len()).is_ok() {
        out.push(format!("{}:", name(actions.len())));
    }
    // The Flash compiler pops a statement's value after the label that skips it, and pops
    // nothing after a statement that leaves nothing: `X, L:, Pop` is `X, Pop, L:` / `X, L:`.
    let mut i = 1;
    while i + 1 < out.len() {
        if out[i].ends_with(':') && out[i + 1] == "Op(23)" {
            let leaves_nothing = matches!(out[i - 1].as_str(), "Op(79)" | "Op(29)" | "Op(60)" | "Op(23)");
            out.remove(i + 1);
            if !leaves_nothing {
                out.insert(i, "Op(23)".into());
            }
        }
        i += 1;
    }
    out
}

fn first_line_diff(a: &str, b: &str) -> (String, String) {
    let (la, lb): (Vec<&str>, Vec<&str>) = (a.lines().collect(), b.lines().collect());
    let i = la.iter().zip(&lb).position(|(x, y)| x != y).unwrap_or(la.len().min(lb.len()));
    (la.get(i).unwrap_or(&"<end>").trim().to_string(), lb.get(i).unwrap_or(&"<end>").trim().to_string())
}

fn first_diff(a: &[u8], b: &[u8]) -> usize {
    a.iter().zip(b).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()))
}

impl fmt::Display for Stats {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "{} scripts ({} bytes): {} not decoded, {} re-encoded differently, {} P-code round trips differ, {} not fully decompiled, {} not recompiled, {} recompiled differently; recompiled bytecode: {} identical, {} the same operations", self.scripts, self.bytes, self.decode_failed, self.encode_diff, self.pcode_diff, self.not_decompiled, self.compile_failed, self.recompile_diff, self.identical, self.same_ops)?;
        for (r, n) in &self.reasons {
            writeln!(f, "  {n:6}  {r}")?;
        }
        Ok(())
    }
}
