//! Source preprocessing: the UnrealScript macro processor
//! (`FMacroProcessingFilter`) and the class factory's text pre-pass
//! (`UClassFactoryUC::FactoryCreateText`) that splits out `cpptext` and
//! `defaultproperties` and scans the class header.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::decl::strip_comments;

#[derive(Clone, Debug)]
pub struct Macro {
    pub params: Vec<String>,
    pub body: String,
}

#[derive(Debug)]
pub struct PreprocessError(pub String);

impl std::fmt::Display for PreprocessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for PreprocessError {}

type PResult<T> = Result<T, PreprocessError>;

/// One input segment on the source stack (a file, an include or a macro expansion).
struct Segment {
    chars: Vec<char>,
    pos: usize,
    is_macro: bool,
    name: String,
    /// Parameter bindings of a macro expansion (`` `param `` resolves here first).
    params: Vec<(String, String)>,
}

struct SourceBuffer {
    stack: Vec<Segment>,
}

impl SourceBuffer {
    fn new() -> Self {
        SourceBuffer { stack: Vec::new() }
    }
    fn push(&mut self, text: &str, is_macro: bool, name: &str) {
        self.stack.push(Segment { chars: text.chars().collect(), pos: 0, is_macro, name: name.to_string(), params: Vec::new() });
    }
    fn push_macro(&mut self, text: &str, name: &str, params: Vec<(String, String)>) {
        self.stack.push(Segment { chars: text.chars().collect(), pos: 0, is_macro: true, name: name.to_string(), params });
    }
    /// Innermost macro parameter binding for `name`.
    fn param(&self, name: &str) -> Option<String> {
        for s in self.stack.iter().rev() {
            if s.is_macro {
                if let Some((_, v)) = s.params.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)) {
                    return Some(v.clone());
                }
            }
        }
        None
    }
    fn has_more(&mut self) -> bool {
        while let Some(top) = self.stack.last() {
            if top.pos < top.chars.len() {
                return true;
            }
            self.stack.pop();
        }
        false
    }
    fn get(&mut self) -> char {
        if !self.has_more() {
            return '\0';
        }
        let top = self.stack.last_mut().unwrap();
        let c = top.chars[top.pos];
        top.pos += 1;
        c
    }
    fn peek(&mut self) -> char {
        self.peek_n(0)
    }
    fn peek_n(&mut self, n: usize) -> char {
        if !self.has_more() {
            return '\0';
        }
        // Peek across segment boundaries is rare; look only in the top segment.
        let top = self.stack.last().unwrap();
        top.chars.get(top.pos + n).copied().unwrap_or('\0')
    }
    fn unget(&mut self, c: char) {
        if c == '\0' {
            return;
        }
        match self.stack.last_mut() {
            Some(top) if top.pos > 0 && top.chars[top.pos - 1] == c => top.pos -= 1,
            _ => self.push(&c.to_string(), false, "unget"),
        }
    }
    fn is_macro(&mut self) -> bool {
        self.has_more() && self.stack.last().map(|s| s.is_macro).unwrap_or(false)
    }
    fn line(&self) -> u32 {
        for s in self.stack.iter().rev() {
            if !s.is_macro {
                return 1 + s.chars[..s.pos].iter().filter(|&&c| c == '\n').count() as u32;
            }
        }
        1
    }
    fn provider(&self) -> String {
        for s in self.stack.iter().rev() {
            if !s.is_macro {
                return s.name.clone();
            }
        }
        String::new()
    }
}

pub struct MacroProcessor {
    pub globals: HashMap<String, Macro>,
    pub locals: HashMap<String, Macro>,
    counters: HashMap<String, i32>,
    enable: Vec<bool>,
    nest_if: i32,
    last_expanded: String,
    src: SourceBuffer,
    initializing_globals: bool,
    /// Directory containing the package directories (`Development/Src`).
    pub source_path: PathBuf,
    pub package_name: String,
    pub class_name: String,
    pub engine_version: u32,
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '#'
}

impl MacroProcessor {
    pub fn new(source_path: &Path, package_name: &str, engine_version: u32) -> Self {
        let mut p = MacroProcessor {
            globals: HashMap::new(),
            locals: HashMap::new(),
            counters: HashMap::new(),
            enable: vec![true],
            nest_if: 0,
            last_expanded: String::new(),
            src: SourceBuffer::new(),
            initializing_globals: false,
            source_path: source_path.to_path_buf(),
            package_name: package_name.to_string(),
            class_name: String::new(),
            engine_version,
        };
        p.init_symbol_table();
        p
    }

    fn set_global(&mut self, name: &str, params: &[&str], body: &str) {
        self.globals.insert(name.to_ascii_lowercase(), Macro { params: params.iter().map(|s| s.to_string()).collect(), body: body.to_string() });
    }

    /// `InitSymbolTable` (release build, no FINAL_RELEASE).
    fn init_symbol_table(&mut self) {
        self.set_global("logd", &[], "");
        self.set_global("Log", &["msg", "cond", "tag"], "`if(`cond)if (`cond) `{endif}LogInternal(`msg`if(`tag),`tag`endif)");
        self.set_global("Warn", &["msg", "cond"], "`if(`cond)if (`cond) `{endif}WarnInternal(`msg)");
        self.set_global("Assert", &["cond"], "Assert(`cond)");
        self.set_global("Date", &[], "\"2026-01-01\"");
        self.set_global("Time", &[], "\"00:00:00\"");
        self.set_global("GameName", &[], "\"Chaos\"");
        self.set_global("Is_ChaosGame", &[], "...");
        let pn = self.package_name.clone();
        self.set_global("PackageName", &[], &pn);
    }

    fn package_path(&self) -> PathBuf {
        self.source_path.join(&self.package_name)
    }
    fn classes_path(&self) -> PathBuf {
        self.package_path().join("Classes")
    }

    /// `ProcessGlobalInclude`: load `<Package>/Globals.uci` definitions.
    pub fn process_global_include(&mut self) -> PResult<()> {
        let pn = self.package_name.clone();
        self.process_global_include_named(&pn)
    }

    /// Load another package's `Globals.uci` into the global symbol table (UDK
    /// keeps every previously compiled package's globals).
    pub fn process_global_include_named(&mut self, package: &str) -> PResult<()> {
        self.initializing_globals = true;
        let mut unused = String::new();
        let pkg_path = self.source_path.join(package);
        let candidates = [pkg_path.join("Globals.uci"), pkg_path.join("Classes").join("Globals.uci")];
        for c in candidates {
            if let Ok(text) = std::fs::read_to_string(&c) {
                let stripped = strip_comments(&text);
                self.src.push(&stripped, false, &c.to_string_lossy());
                self.run(&mut unused)?;
                break;
            }
        }
        self.initializing_globals = false;
        Ok(())
    }

    fn is_output_enabled(&self) -> bool {
        *self.enable.last().unwrap_or(&true)
    }

    /// `Process`: expand macros in `text` (a class file), returning the result.
    pub fn process(&mut self, text: &str, file_name: &str) -> PResult<String> {
        self.locals.clear();
        self.counters.clear();
        self.enable = vec![true];
        self.nest_if = 0;
        let cn = self.class_name.clone();
        self.set_global("ClassName", &[], &cn);
        let mut out = String::new();
        self.src = SourceBuffer::new();
        self.src.push(text, false, file_name);
        self.run(&mut out)?;
        if self.nest_if > 0 {
            return Err(PreprocessError("Unterminated 'if' block".into()));
        }
        Ok(out)
    }

    fn run(&mut self, out: &mut String) -> PResult<()> {
        let mut in_quote = false;
        let mut in_comment = false;
        let mut multi: i32 = 0;
        while self.src.has_more() {
            let ch = self.src.get();
            if ch == '\0' {
                continue;
            }
            if ch == '\\' && in_quote {
                let n = self.src.get();
                if self.is_output_enabled() {
                    out.push(ch);
                    out.push(n);
                }
                continue;
            }
            if ch == '"' && !in_comment && multi == 0 {
                in_quote = !in_quote;
            }
            if ch != '`' || (!self.src.is_macro() && (in_comment || multi > 0 || in_quote)) {
                let next = self.src.peek();
                let is_comment_seq = (ch == '/' && next == '/') || (ch == '/' && next == '*') || (ch == '*' && next == '/');
                if self.is_output_enabled() || ch == '\r' || ch == '\n' || is_comment_seq {
                    if self.is_output_enabled() {
                        out.push(ch);
                    } else if ch == '\n' {
                        out.push(ch);
                    }
                    if (self.is_output_enabled() || is_comment_seq || ch == '\n') && !in_quote {
                        if in_comment {
                            if ch == '\n' {
                                in_comment = false;
                            } else if ch == '*' && next == '/' {
                                multi -= 1;
                                if multi < 0 {
                                    return Err(PreprocessError("Unexpected '*/' outside of comment".into()));
                                }
                                self.src.get();
                                if self.is_output_enabled() {
                                    out.push(next);
                                }
                            } else if ch == '/' && next == '*' {
                                multi += 1;
                                self.src.get();
                                if self.is_output_enabled() {
                                    out.push(next);
                                }
                            }
                        } else if ch == '*' && next == '/' {
                            multi -= 1;
                            if multi < 0 {
                                return Err(PreprocessError("Unexpected '*/' outside of comment".into()));
                            }
                            self.src.get();
                            if self.is_output_enabled() {
                                out.push(next);
                            }
                        } else if ch == '/' {
                            if next == '*' {
                                multi += 1;
                                self.src.get();
                                if self.is_output_enabled() {
                                    out.push(next);
                                }
                            } else if next == '/' {
                                in_comment = true;
                                self.src.get();
                                if self.is_output_enabled() {
                                    out.push(next);
                                }
                            }
                        }
                    }
                }
            } else {
                let name = self.parse_macro_name()?;
                if self.should_process_macro(&name) {
                    self.expand_macro(&name)?;
                }
            }
        }
        Ok(())
    }

    fn should_process_macro(&self, name: &str) -> bool {
        self.is_output_enabled() || matches!(name.to_ascii_lowercase().as_str(), "if" | "elseif" | "else" | "endif" | "ifcondition")
    }

    /// `ParseMacroName`.
    fn parse_macro_name(&mut self) -> PResult<String> {
        let mut wrapped = false;
        let mut out = String::new();
        if self.src.peek() == '{' {
            self.src.get();
            wrapped = true;
        }
        let mut allow_nested = true;
        while self.src.has_more() {
            let ch = self.src.get();
            if ch.is_whitespace() || !is_ident_char(ch) {
                if allow_nested && ch == '`' {
                    let name = self.parse_macro_name()?;
                    if self.should_process_macro(&name) {
                        self.expand_macro(&name)?;
                    }
                    allow_nested = false;
                    continue;
                } else {
                    let mut c = ch;
                    while c.is_whitespace() {
                        c = self.src.get();
                        if c == '\0' {
                            break;
                        }
                    }
                    self.src.unget(c);
                    break;
                }
            }
            if ch != '\0' {
                out.push(ch);
            }
            allow_nested = false;
        }
        if wrapped {
            let ch = self.src.get();
            if ch != '}' {
                return Err(PreprocessError("Unterminated macro name".into()));
            }
        }
        Ok(out)
    }

    /// `ParseMacroDefinition`.
    fn parse_macro_definition(&mut self, name: &str) -> PResult<String> {
        let mut result = String::new();
        let mut span_newline = false;
        let mut wrapped = false;
        let mut count = 0;
        loop {
            let ch = self.src.get();
            if ch == '\0' {
                break;
            }
            if ch == '{' && count == 0 {
                wrapped = true;
                continue;
            } else if ch == '}' && wrapped {
                wrapped = false;
                break;
            }
            count += 1;
            if ch == '\\' {
                if self.src.peek() == 'n' {
                    self.src.get();
                    result.push_str("\r\n");
                    continue;
                }
                span_newline = true;
                continue;
            }
            if ch == '\n' || (ch == '\r' && self.src.peek() == '\n') {
                if span_newline {
                    span_newline = ch == '\r';
                    if !span_newline {
                        let mut c = self.src.get();
                        while c.is_whitespace() && c != '\0' {
                            c = self.src.get();
                        }
                        self.src.unget(c);
                    }
                    continue;
                } else {
                    break;
                }
            } else if span_newline && !ch.is_whitespace() {
                return Err(PreprocessError(format!("Non whitespace character encountered after line-span character in definition for macro '{name}'.")));
            }
            result.push(ch);
        }
        if wrapped {
            return Err(PreprocessError(format!("Failed to find closing '}}' for macro definition of '{name}'")));
        }
        Ok(result)
    }

    /// `ParseNextParam`.
    fn parse_next_param(&mut self) -> PResult<String> {
        let mut param = String::new();
        if !self.src.has_more() {
            return Ok(param);
        }
        let mut ch = self.src.get();
        while ch.is_whitespace() {
            if !self.src.has_more() {
                return Err(PreprocessError("End of script encountered while attempting to parse macro parameter".into()));
            }
            ch = self.src.get();
        }
        let mut escaped = false;
        let mut in_quote = false;
        let mut in_comment = false;
        let mut multi = 0;
        let mut closing = ' ';
        let mut nesting = 1;
        loop {
            if nesting <= 0 {
                break;
            }
            if ch == '\0' && !self.src.has_more() {
                return Err(PreprocessError("End of script encountered while attempting to parse macro parameter".into()));
            }
            let mut processed_comment = false;
            if ch == '/' {
                let next = self.src.peek();
                if next == '*' {
                    multi += 1;
                    self.src.get();
                    processed_comment = true;
                } else if next == '/' {
                    in_comment = true;
                    self.src.get();
                    processed_comment = true;
                }
            } else if in_comment && ch == '\n' {
                in_comment = false;
            } else if multi > 0 && ch == '*' && self.src.peek() == '/' {
                multi -= 1;
                self.src.get();
                processed_comment = true;
            }
            if !in_comment && multi == 0 && !processed_comment {
                if in_quote {
                    if !escaped {
                        if ch == '\\' {
                            escaped = true;
                        } else if ch == closing {
                            in_quote = false;
                        }
                    } else {
                        escaped = false;
                    }
                } else if ch == '"' || ch == '\'' {
                    in_quote = true;
                    closing = ch;
                } else if ch == '(' {
                    nesting += 1;
                } else if ch == ')' || (nesting == 1 && ch == ',') {
                    nesting -= 1;
                    if nesting == 0 {
                        let trimmed = param.trim_end().to_string();
                        param = trimmed;
                        break;
                    }
                }
                if ch != '`' || escaped {
                    if self.is_output_enabled() && ch != '\0' {
                        param.push(ch);
                    }
                } else {
                    let name = self.parse_macro_name()?;
                    if self.should_process_macro(&name) {
                        self.expand_macro(&name)?;
                    }
                }
            }
            ch = self.src.get();
        }
        self.src.unget(ch);
        Ok(param)
    }

    /// `GetParamList`.
    fn get_param_list(&mut self) -> PResult<Vec<String>> {
        let mut params = Vec::new();
        if self.src.peek() != '(' {
            return Ok(params);
        }
        while self.src.peek() != ')' {
            self.src.get();
            let p = self.parse_next_param()?;
            params.push(p);
            if !self.src.has_more() {
                break;
            }
        }
        self.src.get();
        Ok(params)
    }

    fn lookup(&self, name: &str) -> Option<Macro> {
        if let Some(v) = self.src.param(name) {
            return Some(Macro { params: Vec::new(), body: v });
        }
        let key = name.to_ascii_lowercase();
        self.locals.get(&key).cloned().or_else(|| self.globals.get(&key).cloned())
    }

    fn truthy(s: &str) -> bool {
        !s.is_empty() && s != "FALSE" && s != "False" && s != "0"
    }

    /// `ExpandMacro`.
    fn expand_macro(&mut self, name: &str) -> PResult<()> {
        let l = name.to_ascii_lowercase();
        match l.as_str() {
            "if" => {
                if self.src.peek() != '(' {
                    return Err(PreprocessError("Wrong number of parameters for 'if' macro. Expected 1, found 0.".into()));
                }
                while self.src.peek_n(1).is_whitespace() {
                    let open = self.src.get();
                    let _ws = self.src.get();
                    self.src.unget(open);
                }
                if self.src.peek_n(1) != '`' {
                    let params = self.get_param_list()?;
                    let extra = params.first().map(|p| format!(": `if({p})")).unwrap_or_default();
                    return Err(PreprocessError(format!("Syntax error in 'if' macro {extra}")));
                }
                let params = self.get_param_list()?;
                if params.len() != 1 {
                    return Err(PreprocessError(format!("Wrong number of parameters for 'if' macro. Expected 1, found {}.", params.len())));
                }
                self.nest_if += 1;
                let enabled = Self::truthy(&params[0]);
                self.enable.push(enabled);
            }
            "ifcondition" => {
                let params = self.get_param_list()?;
                if params.len() != 1 {
                    return Err(PreprocessError("Wrong number of parameters for 'ifcondition' macro.".into()));
                }
                self.nest_if += 1;
                let enabled = evaluate_if_condition(&params[0])?;
                self.enable.push(enabled);
            }
            "else" => {
                if self.nest_if < 1 || self.enable.len() < 2 || self.last_expanded.eq_ignore_ascii_case("else") {
                    return Err(PreprocessError("Unexpected preprocessor macro 'Else'".into()));
                }
                if self.src.peek() == '(' && !self.get_param_list()?.is_empty() {
                    return Err(PreprocessError("Wrong number of parameters for 'else' macro. Expected 0.".into()));
                }
                if let Some(v) = self.enable.pop() {
                    self.enable.push(!v);
                }
            }
            "elseif" => {
                if self.nest_if < 1 || self.enable.len() < 2 {
                    return Err(PreprocessError("Unexpected preprocessor macro 'ElseIf'".into()));
                }
                let current = *self.enable.last().unwrap();
                *self.enable.last_mut().unwrap() = true;
                if self.src.peek() != '(' {
                    return Err(PreprocessError("Wrong number of parameters for 'elseif' macro. Expected 1.".into()));
                }
                let params = self.get_param_list()?;
                if params.len() != 1 {
                    return Err(PreprocessError("Wrong number of parameters for 'elseif' macro. Expected 1.".into()));
                }
                *self.enable.last_mut().unwrap() = current;
                let enabled = !current && Self::truthy(&params[0]);
                *self.enable.last_mut().unwrap() = enabled;
            }
            "endif" => {
                if self.nest_if < 1 || self.enable.len() < 2 {
                    return Err(PreprocessError("Unexpected preprocessor macro 'EndIf'".into()));
                }
                if self.src.peek() == '(' && !self.get_param_list()?.is_empty() {
                    return Err(PreprocessError("Wrong number of parameters for 'endif' macro. Expected 0.".into()));
                }
                self.nest_if -= 1;
                if self.enable.len() > 1 {
                    self.enable.pop();
                }
            }
            "include" => {
                if self.src.peek() != '(' {
                    return Err(PreprocessError("Wrong number of parameters for 'include' macro. Expected 1.".into()));
                }
                let params = self.get_param_list()?;
                if params.len() != 1 {
                    return Err(PreprocessError("Wrong number of parameters for 'include' macro. Expected 1.".into()));
                }
                self.process_include_file(&params[0])?;
            }
            "define" => {
                let mut ch = self.src.get();
                while ch != '\0' && ch.is_whitespace() {
                    ch = self.src.get();
                }
                if ch == '\0' {
                    return Err(PreprocessError("End of file encountered while searching for name of new macro definition.".into()));
                }
                if !ch.is_ascii_alphabetic() {
                    return Err(PreprocessError("Macro names must begin with an alpha numeric character.".into()));
                }
                let mut new_name = String::new();
                while ch != '\0' && !ch.is_whitespace() && ch != '(' && ch != ')' && ch != '\r' && ch != '\n' {
                    new_name.push(ch);
                    ch = self.src.get();
                }
                if ch == '\0' {
                    return Err(PreprocessError("End of file encountered while parsing name for new macro definition.".into()));
                }
                self.src.unget(ch);
                if new_name == "#" {
                    return Err(PreprocessError("# is a reserved macro name.".into()));
                }
                let params = self.get_param_list()?;
                loop {
                    ch = self.src.get();
                    if ch == '\\' {
                        loop {
                            ch = self.src.get();
                            if ch == '\0' || ch == '\n' {
                                break;
                            }
                        }
                        ch = self.src.get();
                    }
                    if !(ch != '\0' && ch.is_whitespace()) {
                        break;
                    }
                }
                self.src.unget(ch);
                let value = self.parse_macro_definition(&new_name)?;
                let m = Macro { params, body: value };
                if self.initializing_globals {
                    self.globals.insert(new_name.to_ascii_lowercase(), m);
                } else {
                    self.locals.insert(new_name.to_ascii_lowercase(), m);
                }
            }
            "isdefined" | "notdefined" => {
                if self.src.peek() != '(' {
                    return Err(PreprocessError(format!("Wrong number of parameters for '{name}' macro. Expected 1.")));
                }
                let params = self.get_param_list()?;
                if params.len() != 1 {
                    return Err(PreprocessError(format!("Wrong number of parameters for '{name}' macro. Expected 1.")));
                }
                let defined = self.lookup(&params[0]).is_some();
                if defined == (l == "isdefined") {
                    self.src.push("1", true, name);
                }
            }
            "undefine" => {
                let params = self.get_param_list()?;
                if params.len() != 1 {
                    return Err(PreprocessError("Wrong number of parameters for 'undefine' macro. Expected 1.".into()));
                }
                let key = params[0].to_ascii_lowercase();
                if self.locals.remove(&key).is_none() {
                    return Err(PreprocessError(format!("Can't find macro definition to undefine for '{}'", params[0])));
                }
            }
            "counter" | "getcounter" => {
                let params = self.get_param_list()?;
                if params.len() != 1 {
                    return Err(PreprocessError(format!("Wrong number of parameters for '{name}' macro. Expected 1.")));
                }
                let key = params[0].to_ascii_lowercase();
                let v = *self.counters.entry(key.clone()).or_insert(0);
                self.src.push(&v.to_string(), true, name);
                if l == "counter" {
                    self.counters.insert(key, v + 1);
                }
            }
            "setcounter" => {
                let params = self.get_param_list()?;
                if params.len() != 2 {
                    return Err(PreprocessError("Wrong number of parameters for 'setcounter' macro. Expected 2.".into()));
                }
                let v = params[1].trim().parse::<i32>().map_err(|_| PreprocessError("SetCounter: Invalid counter value".into()))?;
                self.counters.insert(params[0].to_ascii_lowercase(), v);
            }
            "engineversion" => {
                let v = self.engine_version.to_string();
                self.src.push(&v, true, name);
            }
            "__line__" => {
                let v = format!("\"{}\"", self.src.line());
                self.src.push(&v, true, name);
            }
            "__file__" => {
                let v = format!("\"{}\"", self.src.provider().replace('\\', "\\\\"));
                self.src.push(&v, true, name);
            }
            _ => {
                let Some(def) = self.lookup(name) else {
                    return Err(PreprocessError(format!("Unknown macro '{name}'.")));
                };
                let body = def.body.clone();
                let mut bindings: Vec<(String, String)> = Vec::new();
                if self.src.peek() == '(' {
                    let params = self.get_param_list()?;
                    for (i, pn) in def.params.iter().enumerate() {
                        bindings.push((pn.clone(), params.get(i).cloned().unwrap_or_default()));
                    }
                } else {
                    bindings.push((name.to_string(), body.clone()));
                    for pn in &def.params {
                        bindings.push((pn.clone(), String::new()));
                    }
                }
                self.src.push_macro(&body, name, bindings);
            }
        }
        self.last_expanded = name.to_string();
        Ok(())
    }

    /// `ProcessIncludeFile`.
    fn process_include_file(&mut self, original: &str) -> PResult<()> {
        let norm = original.replace('\\', "/");
        let no_path = !norm.contains("..") && !norm.contains('/');
        let mut content: Option<(String, PathBuf)> = None;
        if no_path {
            for dir in [self.classes_path(), self.package_path()] {
                let p = dir.join(&norm);
                if let Ok(t) = std::fs::read_to_string(&p) {
                    content = Some((t, p));
                    break;
                }
            }
        }
        if content.is_none() {
            let p = self.source_path.join(&norm);
            if let Ok(t) = std::fs::read_to_string(&p) {
                content = Some((t, p));
            }
        }
        match content {
            Some((text, path)) => {
                let stripped = strip_comments(&text);
                self.src.push(&stripped, false, &path.to_string_lossy());
                Ok(())
            }
            None => {
                if self.initializing_globals {
                    Ok(())
                } else {
                    Err(PreprocessError(format!("Unable to read include file '{original}'")))
                }
            }
        }
    }
}

/// `EvaluteIfCondition` (subset: single values and one binary operator with optional parentheses).
fn evaluate_if_condition(cond: &str) -> PResult<bool> {
    fn value(s: &str) -> PResult<(String, u8)> {
        let s = s.trim();
        if s.starts_with('"') || s.starts_with('\'') {
            return Ok((s[1..s.len().saturating_sub(1)].to_string(), 3));
        }
        if s.eq_ignore_ascii_case("true") {
            return Ok(("True".into(), 0));
        }
        if s.eq_ignore_ascii_case("false") {
            return Ok(("False".into(), 0));
        }
        if s.parse::<i64>().is_ok() {
            return Ok((s.to_string(), 1));
        }
        if s.parse::<f64>().is_ok() {
            return Ok((s.to_string(), 2));
        }
        Err(PreprocessError(format!("Bad expression or missing value: {s}")))
    }
    let c = cond.trim();
    let c = c.strip_prefix('(').and_then(|x| x.strip_suffix(')')).unwrap_or(c).trim();
    for op in ["==", "!=", "<=", ">=", "&&", "||", "^^", "~=", "<", ">"] {
        if let Some(pos) = c.find(op) {
            let (l, r) = (&c[..pos], &c[pos + op.len()..]);
            let (lv, lt) = value(l)?;
            let (rv, rt) = value(r)?;
            return Ok(match (lt, op) {
                (0, "==") => lv == rv,
                (0, "!=") => lv != rv,
                (0, "&&") => lv == "True" && rv == "True",
                (0, "||") => lv == "True" || rv == "True",
                (0, "^^") => (lv == "True") ^ (rv == "True"),
                (3, _) => match op {
                    "==" => lv == rv,
                    "!=" => lv != rv,
                    "~=" => lv.eq_ignore_ascii_case(&rv),
                    "<" => lv < rv,
                    ">" => lv > rv,
                    "<=" => lv <= rv,
                    ">=" => lv >= rv,
                    _ => false,
                },
                _ => {
                    let a: f64 = lv.parse().unwrap_or(0.0);
                    let b: f64 = rv.parse().unwrap_or(0.0);
                    let _ = rt;
                    match op {
                        "==" => a == b,
                        "!=" => a != b,
                        "<" => a < b,
                        ">" => a > b,
                        "<=" => a <= b,
                        ">=" => a >= b,
                        "~=" => (a - b).abs() < 1e-4,
                        _ => false,
                    }
                }
            });
        }
    }
    let (v, t) = value(c)?;
    if t != 0 {
        return Err(PreprocessError("`ifcondition expressions must evalute to a bool value".into()));
    }
    Ok(v == "True")
}

// --------------------------------------------------------------------------
// Class factory pre-pass
// --------------------------------------------------------------------------

/// What `UClassFactoryUC::FactoryCreateText` produces for one `.uc` file.
#[derive(Clone, Debug, Default)]
pub struct ClassSource {
    pub class_name: String,
    pub base_class_name: String,
    pub is_interface: bool,
    /// Processed script text (CRLF line endings, `cpptext`/`defaultproperties` removed).
    pub script_text: String,
    /// `linenumber=N` header followed by the stripped default property lines.
    pub default_prop_text: String,
    pub cpp_text: String,
    pub depends_on: Vec<String>,
}

fn parse_command<'a>(line: &'a str, cmd: &str) -> Option<&'a str> {
    let s = line.trim_start_matches([' ', '\t']);
    if s.len() >= cmd.len() && s.is_char_boundary(cmd.len()) && s[..cmd.len()].eq_ignore_ascii_case(cmd) {
        let rest = &s[cmd.len()..];
        if !rest.chars().next().map(|c| c.is_ascii_alphanumeric()).unwrap_or(false) {
            return Some(rest.trim_start_matches([' ', '\t']));
        }
    }
    None
}

fn line_brace_count(s: &str) -> i32 {
    let mut n = 0;
    let mut quoted = false;
    let mut escaped = false;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            '/' if !quoted && chars.peek() == Some(&'/') => break,
            '{' if !quoted => n += 1,
            '}' if !quoted => n -= 1,
            _ => {}
        }
    }
    n
}

/// Strip comments from one line for header scanning, tracking multi-line comment depth.
fn strip_line_comments(mut line: String, comment_dim: &mut i32) -> (String, bool) {
    let mut str_begin: Option<usize> = None;
    let mut str_end: Option<usize> = None;
    let mut escaped = false;
    for (i, c) in line.char_indices() {
        if escaped {
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == '"' {
            if str_begin.is_none() {
                str_begin = Some(i);
            } else {
                str_end = Some(i);
                break;
            }
        }
    }
    let outside = |pos: usize| str_begin.is_none() || pos < str_begin.unwrap() || pos > str_end.unwrap_or(usize::MAX);
    let mut process = *comment_dim <= 0;
    if let Some(pos) = line.find("//") {
        if outside(pos) {
            line.truncate(pos);
        }
        if line.is_empty() {
            return (line, false);
        }
    }
    let pos = line.find("/*");
    let mut end = line.find("*/");
    if let Some(p) = pos {
        if outside(p) {
            if let Some(e) = end.filter(|&e| outside(e)) {
                line = format!("{}{}", &line[..p], &line[e + 2..]);
                end = None;
            } else {
                line.truncate(p);
                *comment_dim += 1;
            }
        }
        process = *comment_dim <= 1;
    }
    if let Some(e) = end {
        if outside(e) {
            line = line[e + 2..].to_string();
            *comment_dim -= 1;
        }
        process = *comment_dim <= 0;
    }
    (line, process)
}

/// `appStrfind`: case-insensitive search that only matches where the preceding
/// character is not a letter or digit (`_` does count as a lead-in), so the
/// `extends` inside `TgInvListener_CaspianExtendStackLifetime` is not a match.
fn find_word_ci(hay: &str, needle: &str) -> Option<usize> {
    let h = hay.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() || h.len() < n.len() {
        return None;
    }
    let mut alnum = false;
    for i in 0..=h.len() - n.len() {
        let c = h[i];
        if !alnum && h[i..i + n.len()].eq_ignore_ascii_case(n) {
            return Some(i);
        }
        alnum = c.is_ascii_alphanumeric();
    }
    None
}

fn parse_token(s: &str) -> String {
    let s = s.trim_start();
    s.chars().take_while(|c| !c.is_whitespace()).collect()
}

/// Collect the identifiers inside `Keyword(...)` which may span lines.
fn parse_dependent_group(text: &str, keyword: &str, out: &mut Vec<String>) {
    let mut from = 0;
    while let Some(p) = find_word_ci(&text[from..], keyword) {
        let start = from + p + keyword.len();
        let rest = &text[start..];
        let rest_trim = rest.trim_start();
        if let Some(inner) = rest_trim.strip_prefix('(') {
            if let Some(close) = inner.find(')') {
                for name in inner[..close].split(',') {
                    let n = name.trim();
                    if !n.is_empty() && !out.iter().any(|o| o.eq_ignore_ascii_case(n)) {
                        out.push(n.to_string());
                    }
                }
            }
        }
        from = start;
    }
}

/// The factory pre-pass over already macro-expanded text.
pub fn factory_prepass(buffer: &str, expected_name: &str) -> PResult<ClassSource> {
    let mut cs = ClassSource::default();
    let mut script = String::new();
    let mut defaults = String::new();
    let mut cpp = String::new();
    let mut lines = split_lines(buffer).into_iter().peekable();
    let mut current_line: i32 = 0;
    let mut comment_dim = 0;
    let mut got_extends = false;
    let mut class_name = String::new();
    let mut base_name = String::new();
    let mut header_text = String::new();
    let mut in_header = true;
    while let Some(line) = lines.next() {
        current_line += 1;
        let process = comment_dim <= 0;
        if process && parse_command(&line, "cpptext").is_some() {
            if !cpp.is_empty() {
                return Err(PreprocessError("Multiple cpptext definitions".into()));
            }
            script.push_str("// (cpptext)\r\n// (cpptext)\r\n");
            let Some(brace) = lines.next() else { break };
            current_line += 1;
            if !brace.trim_start().starts_with('{') {
                return Err(PreprocessError(format!("Missing '{{' after cpptext in {expected_name}")));
            }
            let mut count = line_brace_count(&brace);
            while let Some(l) = lines.next() {
                current_line += 1;
                script.push_str("// (cpptext)\r\n");
                count += line_brace_count(&l);
                if count == 0 {
                    break;
                }
                cpp.push_str(&l);
                cpp.push_str("\r\n");
            }
        } else if process && parse_command(&line, "defaultproperties").is_some() {
            let mut count = 0;
            while let Some(l) = lines.next() {
                current_line += 1;
                count += line_brace_count(&l);
                if l.contains('{') {
                    defaults.push_str(&format!("linenumber={current_line}\r\n"));
                    break;
                }
            }
            let mut cd = 0;
            while let Some(l) = lines.next() {
                current_line += 1;
                let (stripped, proc_) = strip_line_comments(l.clone(), &mut cd);
                count += line_brace_count(stripped.trim_start());
                if stripped.trim_start().starts_with('}') && proc_ && count == 0 {
                    break;
                }
                if !proc_ || stripped.is_empty() {
                    defaults.push_str("\r\n");
                    continue;
                }
                defaults.push_str(&stripped);
                defaults.push_str("\r\n");
            }
        } else {
            if let Some(rest) = parse_command(&line, "#linenumber") {
                if let Ok(n) = parse_token(rest).parse::<i32>() {
                    current_line = n;
                }
            }
            script.push_str(&line);
            script.push_str("\r\n");
            // Header scan.
            let (sl, proc_) = strip_line_comments(line.clone(), &mut comment_dim);
            if !proc_ || sl.is_empty() {
                continue;
            }
            if class_name.is_empty() {
                if let Some(p) = find_word_ci(&sl, "class") {
                    let after = &sl[p + 5..];
                    if after.starts_with(|c: char| c.is_whitespace()) {
                        class_name = parse_token(after);
                    }
                }
                if class_name.is_empty() {
                    if let Some(p) = find_word_ci(&sl, "interface") {
                        let after = &sl[p + 9..];
                        if after.starts_with(|c: char| c.is_whitespace()) {
                            cs.is_interface = true;
                            class_name = parse_token(after).trim_end_matches(';').to_string();
                        }
                    }
                }
            }
            if in_header {
                header_text.push_str(&sl);
                header_text.push('\n');
            }
            if !got_extends {
                if base_name.is_empty() && !class_name.eq_ignore_ascii_case("Object") && !class_name.eq_ignore_ascii_case("Interface") {
                    if let Some(p) = find_word_ci(&sl, "extends") {
                        got_extends = true;
                        base_name = parse_token(&sl[p + 7..]).trim_end_matches(';').to_string();
                    }
                }
                if !got_extends && sl.contains(';') {
                    got_extends = true;
                }
            }
            if !class_name.is_empty() && sl.contains(';') {
                in_header = false;
            }
        }
    }
    parse_dependent_group(&header_text, "DependsOn", &mut cs.depends_on);
    if !cs.is_interface {
        parse_dependent_group(&header_text, "Implements", &mut cs.depends_on);
    }
    if cs.is_interface && (base_name.is_empty() || base_name.eq_ignore_ascii_case("Object")) {
        base_name = if class_name.eq_ignore_ascii_case("Interface") { "Object".into() } else { "Interface".into() };
    }
    if class_name.is_empty() || (base_name.is_empty() && !class_name.eq_ignore_ascii_case("Object")) {
        return Err(PreprocessError(format!("Bad class definition '{class_name}'/'{base_name}' in {expected_name}")));
    }
    if class_name.eq_ignore_ascii_case(&base_name) {
        return Err(PreprocessError(format!("Class is extending itself '{class_name}'")));
    }
    if !class_name.eq_ignore_ascii_case(expected_name) {
        return Err(PreprocessError(format!("Script vs. class name mismatch ({expected_name}/{class_name})")));
    }
    cs.class_name = class_name;
    cs.base_class_name = base_name;
    cs.script_text = script;
    cs.default_prop_text = defaults;
    cs.cpp_text = cpp;
    Ok(cs)
}

/// `ParseLine(.., Exact=TRUE)` semantics: split on CR, LF or CRLF.
pub fn split_lines(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            out.push(std::mem::take(&mut cur));
        } else if c == '\n' {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}
