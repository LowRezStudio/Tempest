//! UnrealScript lexer mirroring `FScriptCompiler::GetToken`: identifiers,
//! symbols (with the fixed two/three-character set), numeric constants
//! (`+`/`-` prefixes fold into constants unless the caller asks for
//! `no_consts`), name literals, string literals, nested comments and `#`
//! directives.

use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Ident(String),
    Symbol(String),
    Int(i32),
    Float(f32),
    Str(String),
    Name(String),
    Eof,
}

#[derive(Clone, Debug)]
pub struct Token {
    pub tok: Tok,
    pub line: u32,
    /// Byte position of the first character (for backtracking).
    pub start: usize,
    /// Position just after the token.
    pub end: usize,
}

impl Token {
    pub fn is_ident(&self, s: &str) -> bool {
        matches!(&self.tok, Tok::Ident(i) if i.eq_ignore_ascii_case(s))
    }
    pub fn is_symbol(&self, s: &str) -> bool {
        matches!(&self.tok, Tok::Symbol(i) if i == s)
    }
    pub fn ident(&self) -> Option<&str> {
        match &self.tok {
            Tok::Ident(i) => Some(i),
            _ => None,
        }
    }
    pub fn symbol(&self) -> Option<&str> {
        match &self.tok {
            Tok::Symbol(i) => Some(i),
            _ => None,
        }
    }
    pub fn text(&self) -> String {
        match &self.tok {
            Tok::Ident(i) | Tok::Symbol(i) => i.clone(),
            Tok::Int(v) => v.to_string(),
            Tok::Float(v) => v.to_string(),
            Tok::Str(s) => format!("\"{s}\""),
            Tok::Name(n) => format!("'{n}'"),
            Tok::Eof => "<eof>".into(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct LexError {
    pub line: u32,
    pub message: String,
}

impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

pub struct Lexer<'a> {
    src: &'a [u8],
    pub pos: usize,
    pub line: u32,
}

const PAIRS: &[&str] = &[
    "<<", ">>", "!=", "<=", ">=", "++", "--", "+=", "-=", "*=", "/=", "&&", "||", "^^", "==", "**", "~=", "@=", "$=",
];

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Lexer<'a> {
        Lexer { src: src.as_bytes(), pos: 0, line: 1 }
    }
    /// Lex raw bytes (one byte = one `TCHAR`, like the engine's ANSI loading).
    pub fn new_bytes(src: &'a [u8]) -> Lexer<'a> {
        Lexer { src, pos: 0, line: 1 }
    }

    pub fn src(&self) -> &'a [u8] {
        self.src
    }

    pub fn save(&self) -> (usize, u32) {
        (self.pos, self.line)
    }
    pub fn restore(&mut self, s: (usize, u32)) {
        self.pos = s.0;
        self.line = s.1;
    }

    fn peek_byte(&self, off: usize) -> u8 {
        self.src.get(self.pos + off).copied().unwrap_or(0)
    }

    fn bump(&mut self) -> u8 {
        let c = self.peek_byte(0);
        if c != 0 {
            self.pos += 1;
            if c == b'\n' {
                self.line += 1;
            }
        }
        c
    }

    /// Skip whitespace, comments and `#` directives.
    fn skip_trivia(&mut self) -> Result<(), LexError> {
        loop {
            let c = self.peek_byte(0);
            match c {
                b' ' | b'\t' | b'\r' | b'\n' => {
                    self.bump();
                }
                b'/' if self.peek_byte(1) == b'/' => {
                    while !matches!(self.peek_byte(0), 0 | b'\n') {
                        self.bump();
                    }
                }
                b'/' if self.peek_byte(1) == b'*' => {
                    let start_line = self.line;
                    self.bump();
                    self.bump();
                    let mut depth = 1;
                    while depth > 0 {
                        let c = self.bump();
                        if c == 0 {
                            return Err(LexError { line: start_line, message: "End of script encountered inside comment".into() });
                        }
                        if c == b'/' && self.peek_byte(0) == b'*' {
                            self.bump();
                            depth += 1;
                        } else if c == b'*' && self.peek_byte(0) == b'/' {
                            self.bump();
                            depth -= 1;
                        }
                    }
                }
                b'#' => {
                    // compiler directive: consume the line; `#linenumber N` resets the counter
                    let start = self.pos;
                    while !matches!(self.peek_byte(0), 0 | b'\n') {
                        self.bump();
                    }
                    let text = String::from_utf8_lossy(&self.src[start..self.pos]).to_string();
                    let mut words = text[1..].split_whitespace();
                    if words.next().map(|w| w.eq_ignore_ascii_case("linenumber")).unwrap_or(false) {
                        if let Some(n) = words.next().and_then(|w| w.parse::<u32>().ok()) {
                            self.line = n;
                        }
                    }
                }
                _ => return Ok(()),
            }
        }
    }

    pub fn is_eof(&mut self) -> bool {
        let _ = self.skip_trivia();
        self.peek_byte(0) == 0
    }

    /// Next token. With `no_consts`, numeric/name constants are not folded
    /// (so `-1` lexes as `-` then `1`, and `'` is a symbol).
    pub fn next(&mut self, no_consts: bool) -> Result<Token, LexError> {
        self.skip_trivia()?;
        let start = self.pos;
        let line = self.line;
        let c = self.peek_byte(0);
        if c == 0 {
            return Ok(Token { tok: Tok::Eof, line, start, end: start });
        }
        let p = self.peek_byte(1);
        if c.is_ascii_alphabetic() || c == b'_' {
            let mut s = String::new();
            while self.peek_byte(0).is_ascii_alphanumeric() || self.peek_byte(0) == b'_' {
                s.push(self.bump() as char);
            }
            return Ok(Token { tok: Tok::Ident(s), line, start, end: self.pos });
        }
        if !no_consts && (c.is_ascii_digit() || ((c == b'+' || c == b'-') && p.is_ascii_digit())) {
            let mut s = String::new();
            let mut is_float = false;
            let mut is_hex = false;
            s.push(self.bump() as char);
            loop {
                let c = self.peek_byte(0);
                let up = c.to_ascii_uppercase();
                if c.is_ascii_digit() || (!is_float && !is_hex && c == b'.') || (!is_hex && up == b'X') || (is_hex && (b'A'..=b'F').contains(&up)) {
                    if c == b'.' {
                        is_float = true;
                    }
                    if up == b'X' {
                        is_hex = true;
                    }
                    s.push(self.bump() as char);
                } else {
                    break;
                }
            }
            if is_float && matches!(self.peek_byte(0), b'f' | b'F') {
                self.bump();
            }
            let tok = if is_float {
                Tok::Float(s.parse::<f32>().unwrap_or_else(|_| atof(&s)))
            } else if is_hex {
                let digits = s.trim_start_matches(['+', '-']).trim_start_matches("0x").trim_start_matches("0X");
                let v = i64::from_str_radix(digits, 16).unwrap_or(0) as i32;
                Tok::Int(if s.starts_with('-') { v.wrapping_neg() } else { v })
            } else {
                Tok::Int(s.parse::<i64>().unwrap_or(0) as i32)
            };
            return Ok(Token { tok, line, start, end: self.pos });
        }
        if !no_consts && c == b'\'' {
            self.bump();
            let mut s = String::new();
            loop {
                let c = self.peek_byte(0);
                if c.is_ascii_alphanumeric() || c == b'_' || c == b'-' || c == b' ' || c == b'.' {
                    s.push(self.bump() as char);
                } else {
                    break;
                }
            }
            if self.peek_byte(0) != b'\'' {
                return Err(LexError { line, message: "Illegal character in name".into() });
            }
            self.bump();
            return Ok(Token { tok: Tok::Name(s), line, start, end: self.pos });
        }
        if c == b'"' {
            self.bump();
            let mut s = String::new();
            loop {
                let c = self.peek_byte(0);
                if c == b'"' {
                    self.bump();
                    break;
                }
                if c == 0 || c == b'\n' || c == b'\r' {
                    return Err(LexError { line, message: format!("Unterminated string constant: {s}") });
                }
                if c == b'\\' {
                    self.bump();
                    let e = self.peek_byte(0);
                    if e == 0 || e == b'\n' || e == b'\r' {
                        break;
                    }
                    self.bump();
                    s.push(if e == b'n' { '\n' } else { e as char });
                    continue;
                }
                s.push(self.bump() as char);
            }
            return Ok(Token { tok: Tok::Str(s), line, start, end: self.pos });
        }
        // symbol
        let c = self.bump();
        let mut s = String::new();
        s.push(c as char);
        let d = self.peek_byte(0);
        let pair = format!("{}{}", c as char, d as char);
        if PAIRS.contains(&pair.as_str()) {
            self.bump();
            s.push(d as char);
            if c == b'>' && d == b'>' && self.peek_byte(0) == b'>' {
                self.bump();
                s.push('>');
            }
        }
        Ok(Token { tok: Tok::Symbol(s), line, start, end: self.pos })
    }

    /// Raw text up to (not including) `stop` or end of line, trimmed.
    pub fn raw_until(&mut self, stop: u8) -> String {
        let _ = self.skip_trivia();
        let mut s = String::new();
        loop {
            let c = self.peek_byte(0);
            if c == 0 || c == b'\n' || c == b'\r' || c == stop {
                break;
            }
            if c == b'/' && matches!(self.peek_byte(1), b'/' | b'*') {
                break;
            }
            s.push(self.bump() as char);
        }
        s.trim_end().to_string()
    }

    /// Capture the raw source text of a brace-delimited block (after the
    /// opening brace has been consumed), honouring nesting. Returns the text
    /// without the closing brace.
    pub fn raw_block(&mut self) -> Result<String, LexError> {
        let line = self.line;
        let mut depth = 1;
        let mut s = String::new();
        loop {
            let c = self.bump();
            if c == 0 {
                return Err(LexError { line, message: "Unexpected end of file in block".into() });
            }
            if c == b'{' {
                depth += 1;
            } else if c == b'}' {
                depth -= 1;
                if depth == 0 {
                    return Ok(s);
                }
            }
            s.push(c as char);
        }
    }

    pub fn text_between(&self, start: usize, end: usize) -> String {
        String::from_utf8_lossy(&self.src[start.min(self.src.len())..end.min(self.src.len())]).to_string()
    }
}

/// `appAtof`-like lenient float parse.
fn atof(s: &str) -> f32 {
    let mut end = 0;
    let b = s.as_bytes();
    while end < b.len() && (b[end].is_ascii_digit() || matches!(b[end], b'.' | b'-' | b'+' | b'e' | b'E')) {
        end += 1;
    }
    s[..end].parse::<f32>().unwrap_or(0.0)
}
