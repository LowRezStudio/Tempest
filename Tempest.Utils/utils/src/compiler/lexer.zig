// Tokenizer for UnrealScript. Emits a flat token slice over the source text.

const std = @import("std");

pub const TokenKind = enum {
    identifier,
    number, // int or float literal (hex preserved as-is for parsing)
    string, // "..." literal
    name_const, // '...' literal
    object_const, // Class'...' literal (value holds both parts)
    symbol, // any operator or punctuation
    eof,
};

pub const Token = struct {
    kind: TokenKind,
    /// Text of the token (identifier/symbol/number). For string/name/object
    /// consts, the unescaped interior text.
    text: []const u8,
    /// For object_const: the class name before the quote.
    obj_class: []const u8,
    line: u32,
    col: u32,
};

pub const LexError = error{
    UnterminatedString,
    UnterminatedName,
    UnterminatedComment,
    UnexpectedCharacter,
    TooManyTokens,
    NoProgress,
    OutOfMemory,
};

pub const Lexer = struct {
    src: []const u8,
    pos: usize = 0,
    line: u32 = 1,
    col: u32 = 1,

    pub fn init(src: []const u8) Lexer {
        return .{ .src = src };
    }

    fn peek(self: *Lexer) u8 {
        return if (self.pos < self.src.len) self.src[self.pos] else 0;
    }

    fn peekAt(self: *Lexer, off: usize) u8 {
        return if (self.pos + off < self.src.len) self.src[self.pos + off] else 0;
    }

    fn advance(self: *Lexer) u8 {
        const c = self.peek();
        self.pos += 1;
        if (c == '\n') {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        return c;
    }

    fn isIdentStart(c: u8) bool {
        return std.ascii.isAlphabetic(c) or c == '_';
    }

    fn isIdentCont(c: u8) bool {
        return std.ascii.isAlphanumeric(c) or c == '_';
    }

    /// Skip whitespace and comments (both `//` and `/* */`).
    fn skipTrivia(self: *Lexer) LexError!void {
        while (true) {
            while (std.ascii.isWhitespace(self.peek())) _ = self.advance();

            if (self.peek() == '/' and self.peekAt(1) == '/') {
                while (self.peek() != '\n' and self.peek() != 0) _ = self.advance();
                continue;
            }
            if (self.peek() == '/' and self.peekAt(1) == '*') {
                _ = self.advance();
                _ = self.advance();
                while (self.peek() != 0 and !(self.peek() == '*' and self.peekAt(1) == '/')) {
                    _ = self.advance();
                }
                if (self.peek() == 0) return error.UnterminatedComment;
                _ = self.advance(); // *
                _ = self.advance(); // /
                continue;
            }
            return;
        }
    }

    fn symbol(text: []const u8) Token {
        return .{ .kind = .symbol, .text = text, .obj_class = "", .line = 0, .col = 0 };
    }

    /// Recognized multi-character operators, longest first.
    fn matchSymbol(self: *Lexer, buf: []u8) []const u8 {
        const three = [_][]const u8{"<<<"};
        const two = [_][]const u8{
            "<<", ">>", "!=", "<=", ">=", "++", "--", "+=", "-=", "*=", "/=", "&&",
            "||", "^^", "==", "**", "~=", "@=", "$=", ">>>",
        };
        const one = [_][]const u8{
            "(", ")", "[", "]", "{", "}", ",", ";", ".", ":", "?", "=", "<", ">", "+",
            "-", "*", "/", "%", "!", "~", "^", "@", "$", "&", "|", "#",
        };

        for (three) |op| {
            if (self.pos + op.len <= self.src.len and std.mem.eql(u8, self.src[self.pos .. self.pos + op.len], op)) {
                @memcpy(buf[0..op.len], op);
                return buf[0..op.len];
            }
        }
        for (two) |op| {
            if (self.pos + op.len <= self.src.len and std.mem.eql(u8, self.src[self.pos .. self.pos + op.len], op)) {
                @memcpy(buf[0..op.len], op);
                return buf[0..op.len];
            }
        }
        for (one) |op| {
            if (self.peek() == op[0]) {
                buf[0] = op[0];
                return buf[0..1];
            }
        }
        return "";
    }

    fn readIdent(self: *Lexer, start: usize) []const u8 {
        while (isIdentCont(self.peek())) _ = self.advance();
        return self.src[start..self.pos];
    }

    fn readNumber(self: *Lexer, start: usize) []const u8 {
        var is_float = false;
        var is_hex = false;
        // Consume a leading sign (`-1`, `+2`); the tokenizer only enters this
        // branch when a digit follows, so this always consumes at least the
        // digit and therefore at least one byte.
        if (self.peek() == '+' or self.peek() == '-') _ = self.advance();
        while (true) {
            const c = self.peek();
            if (c == '.') {
                is_float = true;
                _ = self.advance();
            } else if (c == 'x' or c == 'X') {
                is_hex = true;
                _ = self.advance();
            } else if (std.ascii.isAlphanumeric(c)) {
                _ = self.advance();
            } else {
                break;
            }
        }
        // Trailing 'f' marks a float literal: "1.5f".
        if ((self.peek() == 'f' or self.peek() == 'F') and is_float) _ = self.advance();
        return self.src[start..self.pos];
    }

    fn readString(self: *Lexer, allocator: std.mem.Allocator, start_line: u32, start_col: u32) LexError!Token {
        _ = self.advance(); // opening quote
        var out = std.ArrayList(u8).empty;
        while (true) {
            const c = self.peek();
            if (c == 0 or c == '\n' or c == '\r') return error.UnterminatedString;
            if (c == '"') {
                _ = self.advance();
                break;
            }
            if (c == '\\') {
                _ = self.advance();
                const e = self.peek();
                if (e == 0) return error.UnterminatedString;
                _ = self.advance();
                switch (e) {
                    'n' => out.append(allocator, '\n') catch return error.OutOfMemory,
                    't' => out.append(allocator, '\t') catch return error.OutOfMemory,
                    'r' => out.append(allocator, '\r') catch return error.OutOfMemory,
                    '\\' => out.append(allocator, '\\') catch return error.OutOfMemory,
                    '"' => out.append(allocator, '"') catch return error.OutOfMemory,
                    else => {
                        out.append(allocator, '\\') catch return error.OutOfMemory;
                        out.append(allocator, e) catch return error.OutOfMemory;
                    },
                }
            } else {
                out.append(allocator, c) catch return error.OutOfMemory;
                _ = self.advance();
            }
        }
        return .{
            .kind = .string,
            .text = out.items,
            .obj_class = "",
            .line = start_line,
            .col = start_col,
        };
    }

    fn readNameConst(self: *Lexer, allocator: std.mem.Allocator, start_line: u32, start_col: u32) LexError!Token {
        _ = self.advance(); // opening quote
        var out = std.ArrayList(u8).empty;
        while (true) {
            const c = self.peek();
            if (c == 0) return error.UnterminatedName;
            if (c == '\'') {
                _ = self.advance();
                break;
            }
            out.append(allocator, c) catch return error.OutOfMemory;
            _ = self.advance();
        }
        return .{
            .kind = .name_const,
            .text = out.items,
            .obj_class = "",
            .line = start_line,
            .col = start_col,
        };
    }

    /// Read an object constant: `Class'Some.Object'`. `class_name` is the
    /// identifier that immediately preceded the quote.
    fn readObjectConst(
        self: *Lexer,
        allocator: std.mem.Allocator,
        class_name: []const u8,
        start_line: u32,
        start_col: u32,
    ) LexError!Token {
        _ = self.advance(); // opening quote
        var out = std.ArrayList(u8).empty;
        while (true) {
            const c = self.peek();
            if (c == 0) return error.UnterminatedName;
            if (c == '\'') {
                _ = self.advance();
                break;
            }
            out.append(allocator, c) catch return error.OutOfMemory;
            _ = self.advance();
        }
        return .{
            .kind = .object_const,
            .text = out.items,
            .obj_class = class_name,
            .line = start_line,
            .col = start_col,
        };
    }

    /// Tokenize the entire source. All token text is copied into the arena.
    pub fn tokenize(self: *Lexer, allocator: std.mem.Allocator) LexError![]Token {
        var toks = std.ArrayList(Token).empty;
        var sym_buf: [4]u8 = undefined;
        // Hard bounds: a real .uc file stays far below these, so hitting one
        // means the lexer failed to make progress (a malformed construct).
        const max_tokens = 1_000_000;
        while (true) {
            if (toks.items.len > max_tokens) return error.TooManyTokens;
            const iter_start = self.pos;
            try self.skipTrivia();
            if (self.peek() == 0) {
                toks.append(allocator, .{ .kind = .eof, .text = "", .obj_class = "", .line = self.line, .col = self.col }) catch return error.OutOfMemory;
                return toks.items;
            }
            const start_line = self.line;
            const start_col = self.col;
            const c = self.peek();

            if (isIdentStart(c)) {
                const start = self.pos;
                const ident = self.readIdent(start);
                // Check for an object constant: Identifier'...
                if (self.peek() == '\'') {
                    toks.append(allocator, try self.readObjectConst(allocator, ident, start_line, start_col)) catch return error.OutOfMemory;
                } else {
                    toks.append(allocator, .{
                        .kind = .identifier,
                        .text = allocator.dupe(u8, ident) catch return error.OutOfMemory,
                        .obj_class = "",
                        .line = start_line,
                        .col = start_col,
                    }) catch return error.OutOfMemory;
                }
            } else if (std.ascii.isDigit(c) or
                ((c == '+' or c == '-') and std.ascii.isDigit(self.peekAt(1))))
            {
                const start = self.pos;
                const num = self.readNumber(start);
                toks.append(allocator, .{
                    .kind = .number,
                    .text = allocator.dupe(u8, num) catch return error.OutOfMemory,
                    .obj_class = "",
                    .line = start_line,
                    .col = start_col,
                }) catch return error.OutOfMemory;
            } else if (c == '"') {
                toks.append(allocator, try self.readString(allocator, start_line, start_col)) catch return error.OutOfMemory;
            } else if (c == '\'') {
                toks.append(allocator, try self.readNameConst(allocator, start_line, start_col)) catch return error.OutOfMemory;
            } else {
                const sym = self.matchSymbol(&sym_buf);
                if (sym.len == 0) {
                    std.debug.print("unexpected char '{c}' at {d}:{d}\n", .{ c, self.line, self.col });
                    return error.UnexpectedCharacter;
                }
                var i: usize = 0;
                while (i < sym.len) : (i += 1) _ = self.advance();
                toks.append(allocator, .{
                    .kind = .symbol,
                    .text = allocator.dupe(u8, sym) catch return error.OutOfMemory,
                    .obj_class = "",
                    .line = start_line,
                    .col = start_col,
                }) catch return error.OutOfMemory;
            }
            // Every token must consume at least one input byte; if not, the
            // tokenizer is spinning and would otherwise allocate forever.
            if (self.pos == iter_start) {
                std.debug.print("lexer no-progress at {d}:{d} char 0x{x}\n", .{ self.line, self.col, c });
                return error.NoProgress;
            }
        }
    }
};
