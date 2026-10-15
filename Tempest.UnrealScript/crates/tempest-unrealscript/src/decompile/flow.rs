//! Statement-level structuring: turns the flat token list of a function or
//! state into `if`/`while`/`for`/`foreach`/`switch` blocks following the
//! exact emission patterns of the UE3 script compiler.

use std::collections::{BTreeMap, HashMap, HashSet};

use super::expr::{Hint, Scope};
use super::Decompiler;
use crate::bytecode::{Script, Statement, Token};

#[derive(Clone, Copy)]
struct LoopCtx {
    break_target: Option<u32>,
    continue_target: Option<u32>,
    /// `foreach`: `continue` is `IteratorNext; Jump(pop)`.
    is_foreach: bool,
}

pub struct Flow<'d, 'a> {
    d: &'d Decompiler<'a>,
    scope: Scope,
    stmts: Vec<Statement>,
    index_of: HashMap<u32, usize>,
    loops: Vec<LoopCtx>,
    /// Labels to print (offset → name): state labels plus synthesized goto targets.
    labels: BTreeMap<u32, String>,
    goto_targets: HashSet<u32>,
    printed_labels: HashSet<u32>,
    out: String,
    indent: usize,
    /// Hint for `return` expressions.
    return_hint: Hint,
}

impl<'d, 'a> Flow<'d, 'a> {
    pub fn new(d: &'d Decompiler<'a>, scope: Scope, script: &Script, labels: BTreeMap<u32, String>, return_hint: Hint) -> Self {
        let mut index_of = HashMap::new();
        for (i, s) in script.statements.iter().enumerate() {
            index_of.insert(s.mem_offset, i);
        }
        index_of.insert(script.mem_size, script.statements.len());
        Flow {
            d,
            scope,
            stmts: script.statements.clone(),
            index_of,
            loops: Vec::new(),
            labels,
            goto_targets: HashSet::new(),
            printed_labels: HashSet::new(),
            out: String::new(),
            indent: 0,
            return_hint,
        }
    }

    /// Render the body at the given indentation level. Runs twice when
    /// unstructured jumps need synthesized labels.
    pub fn render(mut self, indent: usize) -> String {
        self.indent = indent;
        let n = self.stmts.len();
        let end = self.body_end(n);
        self.emit_range(0, end);
        self.emit_trailing_labels(end);
        if !self.goto_targets.is_empty() {
            for &t in &self.goto_targets.clone() {
                self.labels.entry(t).or_insert_with(|| format!("Label_{t:04X}"));
            }
            self.out.clear();
            self.printed_labels.clear();
            self.loops.clear();
            self.indent = indent;
            self.emit_range(0, end);
            self.emit_trailing_labels(end);
        }
        self.out
    }

    /// State labels at or past the end of the code (nothing follows them but the
    /// compiler's own `Stop`): still part of the source.
    fn emit_trailing_labels(&mut self, end: usize) {
        let end_off = self.stmts.get(end).map(|s| s.mem_offset).unwrap_or(u32::MAX);
        let pending: Vec<(u32, String)> = self.labels.iter().filter(|(off, _)| **off >= end_off && !self.printed_labels.contains(off)).map(|(o, l)| (*o, l.clone())).collect();
        for (off, l) in pending {
            self.printed_labels.insert(off);
            let save = self.indent;
            self.indent = self.indent.saturating_sub(1);
            self.line(&format!("{l}:"));
            self.indent = save;
        }
    }

    /// Index just past the last meaningful statement (drops the function
    /// epilogue `Return(Nothing)`/`Return(ReturnNothing)` + `EndOfScript`).
    fn body_end(&self, n: usize) -> usize {
        let mut end = n;
        if end > 0 && matches!(self.stmts[end - 1].token, Token::EndOfScript) {
            end -= 1;
        }
        if end > 0 {
            if let Token::Return(e) = &self.stmts[end - 1].token {
                if matches!(**e, Token::Nothing | Token::ReturnNothing(_)) {
                    end -= 1;
                }
            }
        }
        // State code: skip the label table and its alignment padding, then the `Stop`
        // the compiler appends on its own (jumps may still target it: it stays indexed).
        if self.scope.function.is_none() {
            while end > 0 && matches!(self.stmts[end - 1].token, Token::Nothing | Token::LabelTable(_)) {
                end -= 1;
            }
            if end > 0 && matches!(self.stmts[end - 1].token, Token::Stop) {
                end -= 1;
            }
        }
        end
    }

    fn idx(&self, offset: u32) -> Option<usize> {
        self.index_of.get(&offset).copied()
    }

    fn line(&mut self, s: &str) {
        for _ in 0..self.indent {
            self.out.push_str("    ");
        }
        self.out.push_str(s);
        self.out.push('\n');
    }

    fn expr(&self, t: &Token, hint: Hint) -> String {
        self.d.expr_text(t, hint, self.scope)
    }

    fn cond(&self, t: &Token) -> String {
        self.d.expr_text(t, Hint { enum_: None, bool_ctx: true }, self.scope)
    }

    fn is_break(&self, target: u32) -> bool {
        self.loops.last().map(|l| l.break_target == Some(target)).unwrap_or(false)
    }

    fn is_continue(&self, target: u32) -> bool {
        self.loops.last().map(|l| l.continue_target == Some(target)).unwrap_or(false)
    }

    fn block(&mut self, start: usize, end: usize) {
        self.indent += 1;
        self.emit_range(start, end);
        self.indent -= 1;
    }

    fn emit_range(&mut self, start: usize, end: usize) {
        let mut i = start;
        while i < end {
            let off = self.stmts[i].mem_offset;
            // A statement can head several nested blocks (e.g. `do {`): print its label once.
            if let Some(l) = self.labels.get(&off).cloned() {
                if self.printed_labels.insert(off) {
                    let save = self.indent;
                    self.indent = self.indent.saturating_sub(1);
                    self.line(&format!("{l}:"));
                    self.indent = save;
                }
            }
            i = self.emit_statement(i, end);
        }
    }

    /// Index of the `JumpIfNot` closing a `do { } until ()` loop whose body
    /// starts at `i`, if any (the outermost one when nested).
    fn do_until_end(&self, i: usize, end: usize) -> Option<usize> {
        let off = self.stmts[i].mem_offset;
        let mut found = None;
        for m in i..end {
            if let Token::JumpIfNot { target, .. } = self.stmts[m].token {
                if target as u32 == off && m > i {
                    found = Some(m);
                }
            }
        }
        found
    }

    /// Emit the statement at `i`, return the index of the next one.
    fn emit_statement(&mut self, i: usize, end: usize) -> usize {
        if let Some(m) = self.do_until_end(i, end) {
            let Token::JumpIfNot { cond, .. } = self.stmts[m].token.clone() else { unreachable!() };
            self.line("do");
            self.line("{");
            let break_target = self.stmts.get(m + 1).map(|s| s.mem_offset);
            self.loops.push(LoopCtx { break_target, continue_target: Some(self.stmts[m].mem_offset), is_foreach: false });
            self.block(i, m);
            self.loops.pop();
            self.line(&format!("}} until ({});", self.cond(&cond)));
            return m + 1;
        }
        let tok = self.stmts[i].token.clone();
        let off = self.stmts[i].mem_offset;
        match &tok {
            Token::JumpIfNot { target, cond } => {
                let Some(j) = self.idx(*target as u32) else {
                    self.line(&format!("if (!({})) /* jump {:#x} */;", self.cond(cond), target));
                    return i + 1;
                };
                if j <= i {
                    // backward conditional jump: `do { } until (cond)` was not caught by lookahead
                    let lbl = self.label_for(*target as u32);
                    let c = self.cond(cond);
                    self.line(&format!("if (!({c})) goto {lbl};"));
                    return i + 1;
                }
                let j = j.min(end);
                // loop?
                if j >= i + 2 {
                    if let Token::Jump { target: back } = self.stmts[j - 1].token {
                        if back as u32 == off {
                            return self.emit_loop(i, j, cond);
                        }
                    }
                }
                // if/else?
                if j >= i + 2 && j <= end {
                    if let Token::Jump { target: else_end } = self.stmts[j - 1].token {
                        if (else_end as u32) >= self.stmts[j].mem_offset || (j == end && (else_end as u32) > self.stmts[j - 1].mem_offset) {
                            if let Some(k) = self.idx(else_end as u32) {
                                let empty_else = k == j;
                                if k <= end && !self.is_break(else_end as u32) && !self.is_continue(else_end as u32) && !(empty_else && self.inner_claims(i + 1, j - 1)) {
                                    self.line(&format!("if ({})", self.cond(cond)));
                                    self.line("{");
                                    self.block(i + 1, j - 1);
                                    self.line("}");
                                    self.emit_else(j, k);
                                    return k;
                                }
                            }
                        }
                    }
                }
                self.line(&format!("if ({})", self.cond(cond)));
                self.line("{");
                self.block(i + 1, j);
                self.line("}");
                j
            }
            Token::Jump { target } => {
                if self.is_break(*target as u32) {
                    self.line("break;");
                } else if self.is_continue(*target as u32) {
                    self.line("continue;");
                } else {
                    let l = self.label_for(*target as u32);
                    self.line(&format!("goto {l};"));
                }
                i + 1
            }
            Token::Switch { expr, .. } => self.emit_switch(i, end, expr),
            Token::Case { .. } => {
                // stray case (should have been consumed)
                self.line(&format!("/* stray case {:?} */", tok));
                i + 1
            }
            Token::Iterator { expr, end: loop_end } => {
                let Some(e) = self.idx(*loop_end as u32) else {
                    self.line("/* bad iterator */");
                    return i + 1;
                };
                // `end` is the IteratorPop; stmts[e-1] is the IteratorNext.
                let e = e.min(end);
                let next_idx = e.saturating_sub(1);
                let pop_off = Some(*loop_end as u32);
                self.line(&format!("foreach {}", self.expr(expr, Hint::default())));
                self.line("{");
                self.loops.push(LoopCtx { break_target: pop_off, continue_target: pop_off, is_foreach: true });
                self.block(i + 1, next_idx.max(i + 1));
                self.loops.pop();
                self.line("}");
                if matches!(self.stmts.get(e).map(|s| &s.token), Some(Token::IteratorPop)) { e + 1 } else { e }
            }
            Token::DynArrayIterator { end: loop_end, .. } => {
                let Some(e) = self.idx(*loop_end as u32) else {
                    self.line("/* bad array iterator */");
                    return i + 1;
                };
                let e = e.min(end);
                let next_idx = e.saturating_sub(1);
                let pop_off = Some(*loop_end as u32);
                self.line(&format!("foreach {}", self.expr(&tok, Hint::default())));
                self.line("{");
                self.loops.push(LoopCtx { break_target: pop_off, continue_target: pop_off, is_foreach: true });
                self.block(i + 1, next_idx.max(i + 1));
                self.loops.pop();
                self.line("}");
                if matches!(self.stmts.get(e).map(|s| &s.token), Some(Token::IteratorPop)) { e + 1 } else { e }
            }
            Token::IteratorNext => {
                // `continue` inside foreach: IteratorNext; Jump(pop)
                if let Some(Token::Jump { target }) = self.stmts.get(i + 1).map(|s| &s.token) {
                    if self.loops.last().map(|l| l.is_foreach && l.continue_target == Some(*target as u32)).unwrap_or(false) {
                        self.line("continue;");
                        return i + 2;
                    }
                }
                i + 1
            }
            Token::IteratorPop => {
                // `return` / `break` inside foreach: pops precede the jump/return
                i + 1
            }
            Token::EatReturnValue(_) | Token::Nothing | Token::DebugInfo { .. } | Token::LabelTable(_) | Token::EndOfScript => i + 1,
            Token::Return(e) => {
                let hint = self.return_hint;
                match &**e {
                    Token::Nothing => self.line("return;"),
                    Token::ReturnNothing(_) => {}
                    _ => self.line(&format!("return {};", self.expr(self.d.strip_autocast_to(e, hint), hint))),
                }
                i + 1
            }
            Token::ReturnNothing(_) => i + 1,
            Token::Stop => {
                self.line("stop;");
                i + 1
            }
            Token::GotoLabel(e) => {
                self.line(&format!("goto {};", self.expr(e, Hint::default())));
                i + 1
            }
            Token::Assert { line, .. } => {
                // The assert opcode records its source line: restore it with the
                // `#linenumber` directive (the directive's own line gets that number).
                self.line(&format!("#linenumber {}", line.saturating_sub(1)));
                self.line(&format!("{};", self.expr(&tok, Hint::default())));
                i + 1
            }
            Token::DefaultParmValue { .. } => i + 1,
            Token::JumpIfFilterEditorOnly { target } => {
                let k = self.idx(*target as u32).unwrap_or(i + 1).min(end);
                self.line("filtereditoronly");
                self.line("{");
                self.block(i + 1, k);
                self.line("}");
                k
            }
            Token::Let(..) | Token::LetBool(..) | Token::LetDelegate(..) => {
                // for-loop: Let followed by a loop whose body jumps to its last statement
                if let Some(next) = self.for_loop_shape(i + 1, end) {
                    return self.emit_for(i, next);
                }
                self.line(&format!("{};", self.expr(&tok, Hint::default())));
                i + 1
            }
            _ => {
                let text = self.expr(&tok, Hint::default());
                if !text.is_empty() {
                    self.line(&format!("{text};"));
                }
                i + 1
            }
        }
    }

    /// Case chain of the switch at `s`: `(case index, expr?, target)` list.
    fn switch_cases(&self, s: usize, end: usize) -> Vec<(usize, Option<Token>, Option<u32>)> {
        let mut cases = Vec::new();
        let mut c = s + 1;
        while c < end {
            match &self.stmts[c].token {
                Token::Case { target, expr: e } => {
                    let t = if *target == 0xFFFF { None } else { Some(*target as u32) };
                    cases.push((c, e.as_deref().cloned(), t));
                    match t {
                        Some(t) => match self.idx(t) {
                            Some(n) if n > c => c = n,
                            _ => break,
                        },
                        None => break,
                    }
                }
                _ => break,
            }
        }
        cases
    }

    /// Index just past the switch at `s` whose bodies may extend up to `end`.
    fn switch_end(&self, s: usize, end: usize, cases: &[(usize, Option<Token>, Option<u32>)]) -> usize {
        let last = cases.last().map(|c| c.0).unwrap_or(s);
        if let Some((_, _, Some(t))) = cases.last() {
            return self.idx(*t).map(|n| n.min(end)).unwrap_or(end);
        }
        let default_off = self.stmts[last].mem_offset;
        // Jumps out of the earlier cases that pass the default case are its `break`s:
        // the switch ends where they land (inner if/else jumps stay inside their case).
        let mut from_cases: Option<usize> = None;
        for k in s + 1..last {
            if let Token::Jump { target } = self.stmts[k].token {
                if (target as u32) > default_off {
                    if let Some(n) = self.idx(target as u32) {
                        if n <= end && n > last {
                            from_cases = Some(from_cases.map_or(n, |b| b.max(n)));
                        }
                    }
                }
            }
        }
        if let Some(n) = from_cases {
            return n;
        }
        let mut best: Option<usize> = None;
        for k in s + 1..end {
            if let Token::Jump { target } = self.stmts[k].token {
                if (target as u32) > default_off {
                    if let Some(n) = self.idx(target as u32) {
                        if n <= end && n > last && best.is_none_or(|b| n < b) {
                            best = Some(n);
                        }
                    }
                }
            }
        }
        best.unwrap_or(end)
    }

    /// Would a construct inside `[start, jump]` claim the jump statement at
    /// `jump` (whose target is `jump + 1`)? True when an inner `if` ends
    /// exactly at it (its else-jump) or an inner switch runs through it.
    fn inner_claims(&self, start: usize, jump: usize) -> bool {
        let end_off = self.stmts.get(jump + 1).map(|s| s.mem_offset).unwrap_or(u32::MAX);
        for k in start..jump {
            match &self.stmts[k].token {
                Token::JumpIfNot { target, .. } if *target as u32 == end_off => return true,
                Token::Switch { .. } => {
                    let cases = self.switch_cases(k, jump + 1);
                    if !cases.is_empty() && self.switch_end(k, jump + 1, &cases) == jump + 1 {
                        return true;
                    }
                }
                _ => {}
            }
        }
        false
    }

    fn label_for(&mut self, target: u32) -> String {
        if let Some(l) = self.labels.get(&target) {
            return l.clone();
        }
        self.goto_targets.insert(target);
        format!("Label_{target:04X}")
    }

    fn emit_else(&mut self, j: usize, k: usize) {
        // `else if` chain: a single if statement spanning the whole else block
        if k > j {
            if let Token::JumpIfNot { target, cond } = &self.stmts[j].token.clone() {
                if let Some(t) = self.idx(*target as u32) {
                    // `else { while (...) { ... } }`: the body's final jump goes back to the
                    // condition, which is a loop, not an `else if` chain.
                    let is_loop = t >= j + 2
                        && matches!(self.stmts[t - 1].token, Token::Jump { target: tt } if tt as u32 == self.stmts[j].mem_offset);
                    let chain_end_ok = !is_loop && (t == k
                        || (t >= j + 2
                            && t < k
                            && matches!(self.stmts[t - 1].token, Token::Jump { target: tt } if self.idx(tt as u32) == Some(k))));
                    if chain_end_ok {
                        if t == k {
                            // Last `else if`; a trailing jump to the chain end is an empty `else`.
                            let empty_else = t >= j + 2
                                && matches!(self.stmts[t - 1].token, Token::Jump { target: tt }
                                    if self.idx(tt as u32) == Some(k) && !self.is_break(tt as u32) && !self.is_continue(tt as u32));
                            self.line(&format!("else if ({})", self.cond(cond)));
                            self.line("{");
                            self.block(j + 1, if empty_else { t - 1 } else { t });
                            self.line("}");
                            if empty_else {
                                self.line("else");
                                self.line("{");
                                self.line("}");
                            }
                        } else {
                            self.line(&format!("else if ({})", self.cond(cond)));
                            self.line("{");
                            self.block(j + 1, t - 1);
                            self.line("}");
                            self.emit_else(t, k);
                        }
                        return;
                    }
                }
            }
        }
        self.line("else");
        self.line("{");
        self.block(j, k);
        self.line("}");
    }

    /// Shape of a `JumpIfNot ... Jump back` loop starting at `i`: returns the
    /// index of the back-jump statement if the loop is a `for` (some jump
    /// inside targets the final body statement).
    fn for_loop_shape(&self, i: usize, end: usize) -> Option<usize> {
        let Token::JumpIfNot { target, .. } = &self.stmts.get(i)?.token else { return None };
        let j = self.idx(*target as u32)?;
        if j < i + 3 || j > end {
            return None;
        }
        let Token::Jump { target: back } = self.stmts[j - 1].token else { return None };
        if back as u32 != self.stmts[i].mem_offset {
            return None;
        }
        let inc_off = self.stmts[j - 2].mem_offset;
        // A jump straight to the back-jump would skip the increment: no `for` statement
        // compiles to that (it is an if/else whose else branch is the last statement).
        let back_off = self.stmts[j - 1].mem_offset;
        if (i + 1..j - 2).any(|k| matches!(self.stmts[k].token, Token::Jump { target } if target as u32 == back_off)) {
            return None;
        }
        // The increment must be an affector (assignment / call), and some `continue` must target it,
        // or it must look like a classic increment of the init variable.
        let body_has_continue = (i + 1..j - 2).any(|k| matches!(self.stmts[k].token, Token::Jump { target } if target as u32 == inc_off));
        let inc_is_affector = matches!(
            self.stmts[j - 2].token,
            Token::Let(..) | Token::LetBool(..) | Token::NativeFunction { .. } | Token::FinalFunction { .. } | Token::VirtualFunction { .. }
        );
        if inc_is_affector && (body_has_continue || (i >= 1 && self.looks_like_increment(i - 1, j - 2))) {
            Some(j)
        } else {
            None
        }
    }

    fn looks_like_increment(&self, init: usize, inc: usize) -> bool {
        let init_var = match &self.stmts[init].token {
            Token::Let(a, _) | Token::LetBool(a, _) => self.d.expr_property(a),
            _ => None,
        };
        let inc_var = match &self.stmts[inc].token {
            Token::Let(a, _) | Token::LetBool(a, _) => self.d.expr_property(a),
            Token::NativeFunction { args, .. } | Token::FinalFunction { args, .. } => {
                args.args.first().and_then(|a| self.d.expr_property(a))
            }
            _ => None,
        };
        init_var.is_some() && init_var == inc_var
    }

    fn emit_for(&mut self, init: usize, j: usize) -> usize {
        let cond_idx = init + 1;
        let Token::JumpIfNot { cond, .. } = self.stmts[cond_idx].token.clone() else { unreachable!() };
        let inc = self.stmts[j - 2].token.clone();
        let init_text = self.expr(&self.stmts[init].token.clone(), Hint::default());
        let inc_text = self.expr(&inc, Hint::default());
        self.line(&format!("for ({init_text}; {}; {inc_text})", self.cond(&cond)));
        self.line("{");
        let loop_end = self.stmts[j].mem_offset;
        self.loops.push(LoopCtx { break_target: Some(loop_end), continue_target: Some(self.stmts[j - 2].mem_offset), is_foreach: false });
        self.block(cond_idx + 1, j - 2);
        self.loops.pop();
        self.line("}");
        j
    }

    fn emit_loop(&mut self, i: usize, j: usize, cond: &Token) -> usize {
        // No `for` without an init expression exists in UnrealScript: a loop whose body
        // ends in an "increment" is a `while` (same bytecode).
        self.line(&format!("while ({})", self.cond(cond)));
        self.line("{");
        let loop_end = self.stmts[j].mem_offset;
        self.loops.push(LoopCtx { break_target: Some(loop_end), continue_target: Some(self.stmts[j - 1].mem_offset), is_foreach: false });
        self.block(i + 1, j - 1);
        self.loops.pop();
        self.line("}");
        j
    }

    fn emit_switch(&mut self, i: usize, end: usize, expr: &Token) -> usize {
        let switch_enum = self.d.expr_enum(expr);
        let cases = self.switch_cases(i, end);
        let switch_end = self.switch_end(i, end, &cases);
        let switch_end_off = self.stmts.get(switch_end).map(|s| s.mem_offset).unwrap_or(u32::MAX);
        self.line(&format!("switch ({})", self.expr(expr, Hint::default())));
        self.line("{");
        // `continue` is not allowed inside a switch, so a switch context has no continue target.
        self.loops.push(LoopCtx { break_target: Some(switch_end_off), continue_target: None, is_foreach: false });
        for (n, (cidx, e, _)) in cases.iter().enumerate() {
            let body_end = cases.get(n + 1).map(|c| c.0).unwrap_or(switch_end);
            self.indent += 1;
            match e {
                Some(e) => {
                    let text = self.expr(e, Hint { enum_: switch_enum, bool_ctx: false });
                    self.line(&format!("case {text}:"));
                }
                None => {
                    // Synthesized trailing default with no body is omitted.
                    if body_end == cidx + 1 && n + 1 == cases.len() {
                        self.indent -= 1;
                        continue;
                    }
                    self.line("default:");
                }
            }
            self.block(cidx + 1, body_end);
            self.indent -= 1;
        }
        self.loops.pop();
        self.line("}");
        switch_end
    }
}
