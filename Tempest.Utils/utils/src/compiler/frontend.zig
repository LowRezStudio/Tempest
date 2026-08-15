// Declaration parsing and function-body compilation. A Parser wraps the
// Compiler plus a token cursor and drives both the class-level parsing pass
// and the per-function codegen pass.

const std = @import("std");

const lexer_mod = @import("lexer.zig");
const opcodes = @import("opcodes.zig");
const model = @import("model.zig");
const types = @import("types.zig");
const bytecode = @import("bytecode.zig");
const compile = @import("compile.zig");

const Token = lexer_mod.Token;
const TokenKind = lexer_mod.TokenKind;
const Compiler = compile.Compiler;
const CompileError = compile.CompileError;
const TypeInfo = types.TypeInfo;

/// Case-insensitive ASCII keyword comparison (UnrealScript keywords and
/// operator names are case-insensitive).
fn kwEql(a: []const u8, b: []const u8) bool {
    if (a.len != b.len) return false;
    for (a, b) |ca, cb| {
        if (std.ascii.toLower(ca) != std.ascii.toLower(cb)) return false;
    }
    return true;
}


const Parser = @This();

const PendingLabelJump = struct {
    pos: usize,
    label: []const u8,
};

const LabelEntry = struct {
    name: []const u8,
    code: u16,
};

c: *Compiler,
toks: []const Token,
pos: usize = 0,
/// Active bytecode writer while compiling a function/state body.
w: ?*bytecode.Writer = null,

/// Labels defined in the current function/state, mapped to code positions.
labels: ?*std.StringHashMap(i32) = null,
/// Positions of EX_Jump placeholders that target a not-yet-defined label.
pending_label_jumps: std.ArrayList(PendingLabelJump) = undefined,
/// Positions of unconditional jump placeholders awaiting a patch.
unresolved_jumps: std.ArrayList(usize) = undefined,
/// Label fixups within the current state (label name -> placeholder pos).
label_fixups: ?*std.ArrayList(usize) = null,
/// Tokens of the `for` loop increment expression (compiled after the body).
for_inc_tokens: ?[]const Token = null,
/// Return-value property to destroy after an unused function-call result.
affector_return_prop: ?i32 = null,
/// Loop stack for break/continue resolution.
loops: std.ArrayList(LoopInfo) = undefined,

const LoopInfo = struct {
    break_fixups: std.ArrayList(usize),
    continue_fixups: std.ArrayList(usize),
};

// ---------------------------------------------------------------------------
// Token helpers
// ---------------------------------------------------------------------------

const eof_token = Token{ .kind = .eof, .text = "", .obj_class = "", .line = 0, .col = 0 };

fn peek(self: *Parser) Token {
    if (self.pos >= self.toks.len) return eof_token;
    return self.toks[self.pos];
}

fn peekKind(self: *Parser) TokenKind {
    return self.peek().kind;
}

fn peekSym(self: *Parser) []const u8 {
    const t = self.peek();
    return if (t.kind == .symbol) t.text else "";
}

fn peekIdent(self: *Parser) []const u8 {
    const t = self.peek();
    return if (t.kind == .identifier) t.text else "";
}

fn advance(self: *Parser) Token {
    if (self.pos >= self.toks.len) return eof_token;
    const t = self.toks[self.pos];
    self.pos += 1;
    return t;
}

fn matchSym(self: *Parser, sym: []const u8) bool {
    if (std.mem.eql(u8, self.peekSym(), sym)) {
        self.pos += 1;
        return true;
    }
    return false;
}

fn expectSym(self: *Parser, sym: []const u8) CompileError!void {
    if (!self.matchSym(sym)) {
        return self.errFmt(self.peek(), "expected '{s}', found '{s}'", .{ sym, self.peek().text });
    }
}

fn matchKw(self: *Parser, kw: []const u8) bool {
    if (std.mem.eql(u8, self.peekIdent(), kw)) {
        self.pos += 1;
        return true;
    }
    return false;
}

fn expectKw(self: *Parser, kw: []const u8) CompileError!void {
    if (!self.matchKw(kw)) {
        return self.errFmt(self.peek(), "expected '{s}', found '{s}'", .{ kw, self.peek().text });
    }
}

fn ident(self: *Parser) CompileError![]const u8 {
    const t = self.peek();
    if (t.kind != .identifier) {
        return self.errFmt(t, "expected identifier, found '{s}'", .{t.text});
    }
    self.pos += 1;
    return t.text;
}

fn errFmt(self: *Parser, t: Token, comptime fmt: []const u8, args: anytype) CompileError {
    self.c.err = std.fmt.allocPrint(self.c.allocator, "{s} ({d}:{d}): " ++ fmt, .{ t.text, t.line, t.col } ++ args) catch return error.OutOfMemory;
    self.c.err_line = t.line;
    return error.Syntax;
}

fn here(self: *Parser) Token {
    return self.peek();
}

// ---------------------------------------------------------------------------
// Driver: compile one .uc file into a class object.
// ---------------------------------------------------------------------------

pub fn compileSource(c: *Compiler, src: []const u8, filename: []const u8) (CompileError || lexer_mod.LexError)!*model.ClassObj {
    var lexer = lexer_mod.Lexer.init(src);
    const toks = try lexer.tokenize(c.allocator);
    var p = Parser{ .c = c, .toks = toks };
    defer p.checkTrailing();
    return p.parseClassFile(filename);
}

fn checkTrailing(self: *Parser) void {
    // Tolerate stray semicolons after the class body.
    while (std.mem.eql(u8, self.peekSym(), ";")) self.pos += 1;
    if (self.peekKind() != .eof) {
        std.debug.print("warning: trailing tokens after class body: '{s}'\n", .{self.peek().text});
    }
}

/// Parse a single class declaration file.
fn parseClassFile(self: *Parser, filename: []const u8) CompileError!*model.ClassObj {
    _ = filename;
    // Optionally a leading `class` keyword; some files start directly.
    _ = self.matchKw("class");
    const name = try self.ident();

    if (self.c.findClass(name)) |_| {
        return self.errFmt(self.peek(), "class '{s}' is already defined", .{name});
    }

    const cls = try self.c.allocator.create(model.ClassObj);
    cls.* = .{
        .export_index = self.c.nextExport(),
        .name = name,
        .package_name = self.c.package_name,
        .super_name = "Object",
        .within_name = "Object",
        .config_name = "",
        .class_flags = 0,
        .fields = std.ArrayList(*model.Property).empty,
        .functions = std.ArrayList(*model.Function).empty,
        .states = std.ArrayList(*model.State).empty,
        .enums = std.ArrayList(*model.Enum).empty,
        .structs = std.ArrayList(*model.ScriptStruct).empty,
        .defaults = std.ArrayList(model.DefaultValue).empty,
        .properties_size = 0,
    };
    self.c.classes.append(self.c.allocator, cls) catch return error.OutOfMemory;
    self.c.cur_class = cls;

    try self.parseClassHeading(name);
    // The class body may be wrapped in braces (standard source) or flat
    // (decompiled/exported source).
    const braced = self.matchSym("{");

    // Body: enum/struct/var/function/state declarations until defaultproperties
    // or end of file.
    var need_semicolon: bool = false;
    while (self.peekKind() != .eof) {
        if (self.matchKw("defaultproperties")) {
            try self.parseDefaultProperties(cls);
            break;
        }
        if (self.matchSym("}")) {
            break;
        }
        if (braced and self.peekKind() == .eof) break;
        const before = self.pos;
        try self.parseDeclaration(cls, &need_semicolon);
        _ = self.matchSym(";");
        need_semicolon = false;
        if (self.pos == before) {
            return self.errFmt(self.peek(), "internal error: declaration made no progress", .{});
        }
    }

    // Second pass: compile all deferred function bodies now that the whole
    // class is parsed, so forward references resolve.
    try self.compileDeferredBodies(cls);

    // Class-level flags a script class always has.
    cls.class_flags |= opcodes.class_.compiled | opcodes.class_.parsed;
    if (std.mem.eql(u8, cls.super_name, "Object") or self.c.findClass(cls.super_name) == null) {
        // external superclass: nothing more to inherit
    }

    self.c.cur_class = null;
    return cls;
}

/// Parse `class X extends Y` plus modifiers up to the terminating `;` or `{`.
fn parseClassHeading(self: *Parser, name: []const u8) CompileError!void {
    _ = name;
    const cls = self.c.cur_class.?;
    if (self.matchKw("extends")) {
        cls.super_name = try self.ident();
    }
    if (self.matchKw("within")) {
        cls.within_name = try self.ident();
    }
    // Modifiers.
    while (self.peekKind() == .identifier) {
        const m = self.peekIdent();
        if (kwEql(m, "native")) {
            cls.class_flags |= opcodes.class_.native;
            self.pos += 1;
            // native(UISequence) — consume the optional parenthesized name.
            if (self.matchSym("(")) {
                while (!self.matchSym(")") and self.peekKind() != .eof) self.pos += 1;
            }
        } else if (kwEql(m, "abstract")) {
            cls.class_flags |= opcodes.class_.abstract;
            self.pos += 1;
        } else if (kwEql(m, "transient")) {
            cls.class_flags |= opcodes.class_.transient;
            self.pos += 1;
        } else if (kwEql(m, "config")) {
            cls.class_flags |= opcodes.class_.config;
            self.pos += 1;
            if (self.matchSym("(")) {
                cls.config_name = try self.ident();
                try self.expectSym(")");
            }
        } else if (kwEql(m, "placeable")) {
            cls.class_flags |= opcodes.class_.placeable;
            self.pos += 1;
        } else if (kwEql(m, "notplaceable")) {
            cls.class_flags &= ~opcodes.class_.placeable;
            self.pos += 1;
        } else if (kwEql(m, "editinlinenew")) {
            cls.class_flags |= opcodes.class_.edit_inline_new;
            self.pos += 1;
        } else if (kwEql(m, "collapsecategories")) {
            cls.class_flags |= opcodes.class_.collapse_categories;
            self.pos += 1;
        } else if (kwEql(m, "hidecategories") or kwEql(m, "showcategories") or
            kwEql(m, "dontcollapsecategories") or kwEql(m, "autoexpandcategories") or
            kwEql(m, "autocollapsecategories") or kwEql(m, "dontsortcategories") or
            kwEql(m, "forcescriptorder") or kwEql(m, "dependsOn") or kwEql(m, "dependson") or
            kwEql(m, "perobjectconfig") or kwEql(m, "globalconfig") or
            kwEql(m, "noperobjectconfig") or kwEql(m, "nativereplication"))
        {
            // Accepted and ignored for now (editor-facing).
            self.pos += 1;
            if (self.matchSym("(")) {
                while (!self.matchSym(")") and self.peekKind() != .eof) self.pos += 1;
            }
        } else if (kwEql(m, "implements")) {
            self.pos += 1;
            if (self.matchSym("(")) {
                while (!self.matchSym(")") and self.peekKind() != .eof) self.pos += 1;
            }
        } else {
            break;
        }
    }
    // A heading may end with `;` (class already closed) or `{`.
    _ = self.matchSym(";");
    if (self.peekSym().len > 0 and !std.mem.eql(u8, self.peekSym(), "{")) {
        return self.errFmt(self.peek(), "unexpected '{s}' in class heading", .{self.peek().text});
    }
}

/// Dispatch a class-level declaration.
fn parseDeclaration(self: *Parser, cls: *model.ClassObj, need_semicolon: *bool) CompileError!void {
    const t = self.peek();
    if (t.kind != .identifier) {
        return self.errFmt(t, "unexpected token '{s}'", .{t.text});
    }
    const kw = t.text;

    if (kwEql(kw, "enum")) {
        self.pos += 1;
        const e = try self.parseEnum(cls);
        cls.enums.append(self.c.allocator, e) catch return error.OutOfMemory;
    } else if (kwEql(kw, "struct")) {
        self.pos += 1;
        const s = try self.parseStruct(cls);
        cls.structs.append(self.c.allocator, s) catch return error.OutOfMemory;
    } else if (kwEql(kw, "var")) {
        self.pos += 1;
        try self.parseVarDecl(cls);
    } else if (kwEql(kw, "const")) {
        // Named constants are folded at use sites; skip the declaration.
        self.pos += 1;
        _ = try self.ident();
        _ = self.matchSym("=");
        // Skip until semicolon at depth 0.
        while (self.peekKind() != .eof and !self.matchSym(";")) self.pos += 1;
    } else if (kwEql(kw, "function") or kwEql(kw, "event") or
        kwEql(kw, "native") or kwEql(kw, "final") or
        kwEql(kw, "static") or kwEql(kw, "exec") or
        kwEql(kw, "iterator") or kwEql(kw, "latent") or
        kwEql(kw, "singular") or kwEql(kw, "simulated") or
        kwEql(kw, "operator") or kwEql(kw, "preoperator") or
        kwEql(kw, "postoperator") or kwEql(kw, "delegate") or
        kwEql(kw, "private") or kwEql(kw, "protected") or
        kwEql(kw, "public"))
    {
        const f = try self.parseFunction(cls, need_semicolon);
        if (f) |fn_obj| cls.functions.append(self.c.allocator, fn_obj) catch return error.OutOfMemory;
    } else if (kwEql(kw, "state")) {
        self.pos += 1;
        const s = try self.parseState(cls);
        cls.states.append(self.c.allocator, s) catch return error.OutOfMemory;
        need_semicolon.* = false;
    } else if (kwEql(kw, "ignores")) {
        // `ignores FuncName;` - accepted and ignored.
        self.pos += 1;
        while (self.peekKind() != .eof and !self.matchSym(";")) self.pos += 1;
    } else {
        return self.errFmt(t, "unexpected declaration '{s}'", .{t.text});
    }
}

/// Parse `enum Name { A, B, C }`.
fn parseEnum(self: *Parser, cls: *model.ClassObj) CompileError!*model.Enum {
    _ = cls;
    const name = try self.ident();
    var e = self.c.allocator.create(model.Enum) catch return error.OutOfMemory;
    e.* = .{ .export_index = self.c.nextExport(), .name = name, .values = &.{} };
    var vals = std.ArrayList([]const u8).empty;
    if (self.matchSym("{")) {
        while (self.peekKind() != .eof and !self.matchSym("}")) {
            if (self.matchSym(",")) continue;
            const v = try self.ident();
            vals.append(self.c.allocator, v) catch return error.OutOfMemory;
        }
    }
    _ = self.matchSym(";");
    e.values = vals.items;
    _ = cls;
    return e;
}

/// Consume a balanced `[ ... ]` block at the current position (peek must be
/// `[`), honoring nested brackets.
fn skipBalancedBrackets(self: *Parser) CompileError!void {
    if (!self.matchSym("[")) return;
    var depth: usize = 1;
    while (self.peekKind() != .eof and depth > 0) {
        if (std.mem.eql(u8, self.peekSym(), "[")) {
            depth += 1;
            self.pos += 1;
        } else if (std.mem.eql(u8, self.peekSym(), "]")) {
            depth -= 1;
            self.pos += 1;
        } else {
            self.pos += 1;
        }
    }
}

/// Consume a balanced `{ ... }` block at the current position (peek must be
/// `{`), honoring nested braces.
fn skipBalancedBraces(self: *Parser) CompileError!void {
    if (!self.matchSym("{")) return;
    var depth: usize = 1;
    while (self.peekKind() != .eof and depth > 0) {
        if (std.mem.eql(u8, self.peekSym(), "{")) {
            depth += 1;
            self.pos += 1;
        } else if (std.mem.eql(u8, self.peekSym(), "}")) {
            depth -= 1;
            self.pos += 1;
        } else {
            self.pos += 1;
        }
    }
}

/// Parse `struct [modifiers] Name [modifiers] { var ... }`.
fn parseStruct(self: *Parser, cls: *model.ClassObj) CompileError!*model.ScriptStruct {
    var flags: u32 = 0;
    // Modifiers may precede the name (`struct native ExternalTexture`).
    while (self.peekKind() == .identifier) {
        const m = self.peekIdent();
        if (kwEql(m, "native")) {
            flags |= opcodes.struct_.native;
            self.pos += 1;
        } else if (kwEql(m, "transient")) {
            flags |= opcodes.struct_.transient;
            self.pos += 1;
        } else if (kwEql(m, "noexport")) {
            self.pos += 1;
        } else {
            break;
        }
    }
    const name = try self.ident();
    if (self.matchKw("native")) flags |= opcodes.struct_.native;
    if (self.matchKw("transient")) flags |= opcodes.struct_.transient;
    var s = self.c.allocator.create(model.ScriptStruct) catch return error.OutOfMemory;
    s.* = .{
        .export_index = self.c.nextExport(),
        .name = name,
        .flags = flags,
        .super_name = "",
        .fields = std.ArrayList(*model.Property).empty,
        .offset = 0,
    };
    if (self.matchSym("{")) {
        while (self.peekKind() != .eof and !self.matchSym("}")) {
            if (self.matchKw("var")) {
                const props = try self.parseVarDeclList(s);
                for (props) |p| {
                    s.fields.append(self.c.allocator, p) catch return error.OutOfMemory;
                }
            } else if (std.mem.eql(u8, self.peekSym(), "{")) {
                // A nested block such as `structdefaultproperties { ... }`;
                // skip it as a unit so its closing brace is not taken for the
                // struct's own terminator.
                try self.skipBalancedBraces();
            } else {
                self.pos += 1;
            }
        }
    }
    _ = self.matchSym(";");
    _ = cls;
    return s;
}

/// Parse `var [modifiers] Type VarName [= default] [, VarName2 ...];`.
/// Returns the created properties (appended to `owner`'s field list by caller
/// when owner is a class; struct owns its own list).
fn parseVarDecl(self: *Parser, cls: *model.ClassObj) CompileError!void {
    const props = try self.parseVarDeclList(cls);
    for (props) |p| {
        cls.fields.append(self.c.allocator, p) catch return error.OutOfMemory;
    }
}

/// Parse one `var` declaration (after the `var` keyword), returning the list of
/// properties. `scope` is the owning class or struct (for validation).
fn parseVarDeclList(self: *Parser, scope: anytype) CompileError![]*model.Property {
    _ = scope;
    var flags: u64 = 0;
    var obj_flags: u64 = 0;

    // Optional editor category: `var(...)` / `var(SomeCategory)`.
    if (self.matchSym("(")) {
        while (!self.matchSym(")") and self.peekKind() != .eof) self.pos += 1;
    }

    // Property modifiers.
    while (self.peekKind() == .identifier) {
        const m = self.peekIdent();
        if (kwEql(m, "const")) {
            flags |= opcodes.cp.const_;
            self.pos += 1;
        } else if (kwEql(m, "config")) {
            flags |= opcodes.cp.config;
            self.pos += 1;
        } else if (kwEql(m, "globalconfig")) {
            flags |= opcodes.cp.config | opcodes.cp.global_config;
            self.pos += 1;
        } else if (kwEql(m, "localized")) {
            flags |= opcodes.cp.localized;
            self.pos += 1;
        } else if (kwEql(m, "private")) {
            obj_flags |= opcodes.rf.protected_;
            self.pos += 1;
        } else if (kwEql(m, "protected")) {
            obj_flags |= opcodes.rf.protected_;
            self.pos += 1;
        } else if (kwEql(m, "editconst")) {
            flags |= opcodes.cp.edit_const;
            self.pos += 1;
        } else if (kwEql(m, "transient")) {
            flags |= opcodes.cp.transient;
            self.pos += 1;
        } else if (kwEql(m, "native")) {
            flags |= opcodes.cp.native;
            self.pos += 1;
        } else if (kwEql(m, "noexport")) {
            flags |= opcodes.cp.no_export;
            self.pos += 1;
        } else if (kwEql(m, "editinline")) {
            flags |= opcodes.cp.edit_inline;
            self.pos += 1;
        } else if (kwEql(m, "editinlineuse")) {
            flags |= opcodes.cp.edit_inline | opcodes.cp.edit_inline_use;
            self.pos += 1;
        } else if (kwEql(m, "noclear")) {
            flags |= opcodes.cp.no_clear;
            self.pos += 1;
        } else if (kwEql(m, "editfixedarray") or kwEql(m, "editfixedsize")) {
            flags |= opcodes.cp.edit_fixed_size;
            self.pos += 1;
        } else if (kwEql(m, "input")) {
            flags |= opcodes.cp.input;
            self.pos += 1;
        } else if (kwEql(m, "repnotify")) {
            flags |= opcodes.cp.rep_notify;
            self.pos += 1;
        } else if (kwEql(m, "interp")) {
            flags |= opcodes.cp.edit | opcodes.cp.interp;
            self.pos += 1;
        } else if (kwEql(m, "deprecated")) {
            flags |= opcodes.cp.deprecated;
            self.pos += 1;
        } else if (kwEql(m, "duplicatetransient")) {
            flags |= opcodes.cp.duplicate_transient;
            self.pos += 1;
        } else if (std.mem.eql(u8, m, "nonTransactional")) {
            flags |= opcodes.cp.non_transactional;
            self.pos += 1;
        } else if (kwEql(m, "editoronly")) {
            flags |= opcodes.cp.editor_only;
            self.pos += 1;
        } else if (kwEql(m, "notforconsole")) {
            flags |= opcodes.cp.not_for_console;
            self.pos += 1;
        } else if (kwEql(m, "privatewrite")) {
            flags |= opcodes.cp.private_write;
            self.pos += 1;
        } else if (kwEql(m, "protectedwrite")) {
            flags |= opcodes.cp.protected_write;
            self.pos += 1;
        } else if (kwEql(m, "out")) {
            flags |= opcodes.cp.out_parm | opcodes.cp.parm;
            self.pos += 1;
        } else if (kwEql(m, "optional")) {
            flags |= opcodes.cp.optional_parm | opcodes.cp.parm;
            self.pos += 1;
        } else if (kwEql(m, "coerce")) {
            flags |= opcodes.cp.coerce_parm | opcodes.cp.parm;
            self.pos += 1;
        } else if (kwEql(m, "init")) {
            flags |= opcodes.cp.always_init;
            self.pos += 1;
        } else if (kwEql(m, "instanced")) {
            flags |= opcodes.cp.edit_inline | opcodes.cp.export_object;
            self.pos += 1;
        } else if (kwEql(m, "databinding")) {
            flags |= opcodes.cp.data_binding;
            self.pos += 1;
        } else if (kwEql(m, "repretry")) {
            flags |= opcodes.cp.rep_retry;
            self.pos += 1;
        } else if (kwEql(m, "crosslevelpassive")) {
            flags |= opcodes.cp.cross_level_passive;
            self.pos += 1;
        } else if (kwEql(m, "crosslevelactive")) {
            flags |= opcodes.cp.cross_level_active;
            self.pos += 1;
        } else if (kwEql(m, "serializetext")) {
            flags |= opcodes.cp.serialize_text;
            self.pos += 1;
        } else if (kwEql(m, "export")) {
            flags |= opcodes.cp.export_object;
            self.pos += 1;
        } else if (kwEql(m, "skip")) {
            flags |= opcodes.cp.skip_parm | opcodes.cp.parm;
            self.pos += 1;
        } else if (kwEql(m, "edittextbox")) {
            flags |= opcodes.cp.edit_text_box;
            self.pos += 1;
        } else {
            break;
        }
    }

    // Type.
    const ti = try self.parseType();

    // Names (comma-separated), with optional static array dims.
    var props = std.ArrayList(*model.Property).empty;
    while (true) {
        const varname = try self.ident();
        var array_dim: i32 = ti.array_dim;
        if (self.matchSym("[")) {
            if (ti.isDynamicArray()) return self.errFmt(self.peek(), "arrays within arrays not supported", .{});
            const dim_tok = self.peek();
            if (dim_tok.kind == .number) {
                const n = try self.parseInt(dim_tok.text);
                array_dim = @intCast(n);
                self.pos += 1;
            } else if (dim_tok.kind == .identifier) {
                // enum-based size: array[EnumType]
                const en = try self.resolveEnum(dim_tok.text);
                array_dim = @intCast(en.values.len);
                self.pos += 1;
            } else {
                return self.errFmt(dim_tok, "expected array size", .{});
            }
            try self.expectSym("]");
        }

        // Native properties may carry C++ export text `{ ... }` after the name.
        if (std.mem.eql(u8, self.peekSym(), "{")) try self.skipBalancedBraces();

        const prop = try self.c.allocator.create(model.Property);
        prop.* = .{
            .export_index = self.c.nextExport(),
            .name = varname,
            .prop_type = ti.prop_type,
            .flags = flags,
            .array_dim = array_dim,
            .array_size_enum = ti.enum_,
            .enum_ = ti.enum_,
            .property_class = ti.property_class,
            .meta_class = ti.meta_class,
            .struct_ = ti.struct_,
            .inner = try self.typeInfoToInner(ti),
            .delegate_function = ti.delegate_function,
            .is_param = false,
            .is_return = false,
            .is_out = (flags & opcodes.cp.out_parm) != 0,
            .is_optional = (flags & opcodes.cp.optional_parm) != 0,
            .is_coerce = (flags & opcodes.cp.coerce_parm) != 0,
            .is_const = (flags & opcodes.cp.const_) != 0,
            .is_skip = (flags & opcodes.cp.skip_parm) != 0,
            .optional_default = null,
            .offset = 0,
            .next = null,
            .element_size = ti.size(),
        };
        props.append(self.c.allocator, prop) catch return error.OutOfMemory;

        // Optional `= default` on the last name of a class member.
        if (self.matchSym("=")) {
            const start = self.pos;
            var depth: usize = 0;
            while (self.peekKind() != .eof) {
                if (self.peekSym().len == 0 and self.peekKind() != .eof) break;
                if (std.mem.eql(u8, self.peekSym(), "(") or std.mem.eql(u8, self.peekSym(), "[") or
                    std.mem.eql(u8, self.peekSym(), "{")) depth += 1;
                if (std.mem.eql(u8, self.peekSym(), ")") or std.mem.eql(u8, self.peekSym(), "]") or
                    std.mem.eql(u8, self.peekSym(), "}")) {
                    if (depth == 0) break;
                    depth -= 1;
                }
                if (depth == 0 and std.mem.eql(u8, self.peekSym(), ",")) break;
                if (depth == 0 and std.mem.eql(u8, self.peekSym(), ";")) break;
                self.pos += 1;
            }
            const value = self.srcSlice(start, self.pos);
            _ = value;
        }

        if (!self.matchSym(",")) break;
    }
    return props.items;
}

fn srcSlice(self: *Parser, start: usize, end: usize) []const u8 {
    // Reconstruct from token positions is complex; store nothing for now.
    _ = self;
    _ = start;
    _ = end;
    return "";
}

/// Build a Property from a TypeInfo, for use as a dynamic array inner element.
fn typeInfoToInner(self: *Parser, ti: TypeInfo) CompileError!?*model.Property {
    if (ti.array_dim != 0) return null; // not a dynamic array
    if (ti.prop_type == .none) return null;
    const p = try self.c.allocator.create(model.Property);
    p.* = .{
        .export_index = self.c.nextExport(),
        .name = "Element",
        .prop_type = ti.prop_type,
        .flags = 0,
        .array_dim = 1,
        .array_size_enum = ti.enum_,
        .enum_ = ti.enum_,
        .property_class = ti.property_class,
        .meta_class = ti.meta_class,
        .struct_ = ti.struct_,
        .inner = null,
        .delegate_function = ti.delegate_function,
        .is_param = false,
        .is_return = false,
        .is_out = false,
        .is_optional = false,
        .is_coerce = false,
        .is_const = false,
        .is_skip = false,
        .optional_default = null,
        .offset = 0,
        .next = null,
        .element_size = ti.size(),
    };
    return p;
}

fn parseInt(self: *Parser, text: []const u8) CompileError!i64 {
    if (text.len > 2 and (std.mem.eql(u8, text[0..2], "0x") or std.mem.eql(u8, text[0..2], "0X"))) {
        return std.fmt.parseInt(i64, text[2..], 16) catch {
            return self.errFmt(self.peek(), "bad integer literal '{s}'", .{text});
        };
    }
    return std.fmt.parseInt(i64, text, 10) catch {
        return self.errFmt(self.peek(), "bad integer literal '{s}'", .{text});
    };
}

fn parseFloat(self: *Parser, text: []const u8) CompileError!f64 {
    var buf = self.c.allocator.dupe(u8, text) catch return error.OutOfMemory;
    defer self.c.allocator.free(buf);
    if (buf.len > 0 and (buf[buf.len - 1] == 'f' or buf[buf.len - 1] == 'F')) buf = buf[0 .. buf.len - 1];
    return std.fmt.parseFloat(f64, buf) catch {
        return self.errFmt(self.peek(), "bad float literal '{s}'", .{text});
    };
}

/// Consume property modifiers that may prefix an array inner type
/// (`array<editconst string>`, `array<editinline Object>`).
fn skipInnerModifiers(self: *Parser) CompileError!void {
    const mods = [_][]const u8{
        "editconst", "editinline", "editinlineuse", "const", "config",
        "transient", "native", "noexport", "noclear", "editfixedarray",
        "editfixedsize", "deprecated", "duplicatetransient", "localized",
        "notforconsole", "privatewrite", "protectedwrite",
    };
    while (self.peekKind() == .identifier) {
        var matched = false;
        for (mods) |m| {
            if (std.mem.eql(u8, self.peekIdent(), m)) {
                self.pos += 1;
                matched = true;
                break;
            }
        }
        if (!matched) break;
    }
}

/// Resolve a type reference to a TypeInfo.
fn parseType(self: *Parser) CompileError!TypeInfo {
    const t = try self.ident();
    var ti: TypeInfo = .{};

    if (kwEql(t, "byte")) {
        ti.prop_type = .byte;
        // Optional enum type after byte.
        if (self.matchSym("<")) {
            const en = try self.ident();
            ti.enum_ = try self.resolveEnum(en);
            try self.expectSym(">");
        }
    } else if (kwEql(t, "int")) {
        ti.prop_type = .int_;
    } else if (kwEql(t, "bool")) {
        ti.prop_type = .bool_;
    } else if (kwEql(t, "float")) {
        ti.prop_type = .float_;
    } else if (kwEql(t, "string")) {
        ti.prop_type = .string;
        // obsolete `string[Size]` — accept and ignore.
        if (self.matchSym("[")) {
            while (!self.matchSym("]") and self.peekKind() != .eof) self.pos += 1;
        }
    } else if (kwEql(t, "name")) {
        ti.prop_type = .name;
    } else if (kwEql(t, "vector")) {
        ti = try self.builtinStruct("Vector");
    } else if (kwEql(t, "rotator")) {
        ti = try self.builtinStruct("Rotator");
    } else if (kwEql(t, "class")) {
        ti.prop_type = .object_reference;
        ti.property_class = "Class";
        if (self.matchSym("<")) {
            ti.meta_class = try self.ident();
            try self.expectSym(">");
        }
    } else if (kwEql(t, "array")) {
        try self.expectSym("<");
        try self.skipInnerModifiers();
        const inner_ti = try self.parseType();
        try self.expectSym(">");
        // Inner cannot itself be a dynamic array.
        if (inner_ti.isDynamicArray()) return self.errFmt(self.peek(), "arrays within arrays not supported", .{});
        const inner_copy = self.c.allocator.create(TypeInfo) catch return error.OutOfMemory;
        inner_copy.* = inner_ti;
        ti = .{ .prop_type = inner_ti.prop_type, .array_dim = 0, .inner = inner_copy };
        ti.enum_ = inner_ti.enum_;
        ti.property_class = inner_ti.property_class;
        ti.meta_class = inner_ti.meta_class;
        ti.struct_ = inner_ti.struct_;
    } else if (kwEql(t, "delegate")) {
        ti.prop_type = .delegate;
        if (self.matchSym("<")) {
            ti.delegate_function = try self.ident();
            try self.expectSym(">");
        }
    } else if (kwEql(t, "map")) {
        // map<K,V> only in native classes; store as opaque. Native maps carry
        // their key/value types as export text `{K,V}`.
        ti.prop_type = .map;
        if (std.mem.eql(u8, self.peekSym(), "{")) try self.skipBalancedBraces();
    } else if (kwEql(t, "object")) {
        ti.prop_type = .object_reference;
        ti.property_class = "Object";
    } else if (self.c.findClass(t)) |c| {
        ti.prop_type = .object_reference;
        ti.property_class = c.name;
    } else if (self.c.findEnum(t)) |e| {
        ti.prop_type = .byte;
        ti.enum_ = e;
    } else if (self.c.findStruct(t)) |s| {
        ti.prop_type = .struct_;
        ti.struct_ = s;
    } else {
        // Unknown object type: assume a Core/Engine class reference.
        ti.prop_type = .object_reference;
        ti.property_class = t;
    }

    // Qualifier: ClassName.TypeName (enum/struct in another class).
    if (self.matchSym(".")) {
        const member = try self.ident();
        if (self.c.findClass(t)) |c| {
            if (c.findEnum(member)) |e| {
                ti.prop_type = .byte;
                ti.enum_ = e;
            } else if (c.findStruct(member)) |s| {
                ti.prop_type = .struct_;
                ti.struct_ = s;
            } else {
                return self.errFmt(self.peek(), "unknown type '{s}.{s}'", .{ t, member });
            }
        }
    }
    return ti;
}

fn builtinStruct(self: *Parser, name: []const u8) CompileError!TypeInfo {
    const s = self.c.allocator.create(model.ScriptStruct) catch return error.OutOfMemory;
    s.* = .{
        .name = name,
        .flags = 0,
        .super_name = "",
        .fields = std.ArrayList(*model.Property).empty,
        .offset = if (std.mem.eql(u8, name, "Vector")) 12 else 12,
    };
    return .{ .prop_type = .struct_, .struct_ = s };
}

fn resolveEnum(self: *Parser, name: []const u8) CompileError!*model.Enum {
    if (self.c.findEnum(name)) |e| return e;
    if (self.c.cur_class) |cls| {
        if (cls.findEnum(name)) |e| return e;
    }
    return self.errFmt(self.peek(), "unknown enum '{s}'", .{name});
}

// ---------------------------------------------------------------------------
// Function declarations
// ---------------------------------------------------------------------------

/// Parse a function declaration (or return null for a `delegate` declaration
/// that only binds a type). `need_semicolon` reports whether the caller must
/// consume a `;`.
fn parseFunction(self: *Parser, cls: *model.ClassObj, need_semicolon: *bool) CompileError!?*model.Function {
    _ = cls;
    var flags: u32 = 0;
    var i_native: i32 = 0;
    var oper_precedence: i32 = 0;
    var is_operator = false;
    var is_pre = false;
    var is_post = false;
    // Function modifiers.
    while (self.peekKind() == .identifier) {
        const m = self.peekIdent();
        if (kwEql(m, "native")) {
            flags |= opcodes.func.native;
            self.pos += 1;
            if (self.matchSym("(")) {
                i_native = @intCast(try self.parseInt(try self.ident()));
                try self.expectSym(")");
            }
        } else if (kwEql(m, "event")) {
            flags |= opcodes.func.event;
            self.pos += 1;
        } else if (kwEql(m, "static")) {
            flags |= opcodes.func.static_;
            self.pos += 1;
        } else if (kwEql(m, "final")) {
            flags |= opcodes.func.final_;
            self.pos += 1;
        } else if (kwEql(m, "exec")) {
            flags |= opcodes.func.exec;
            self.pos += 1;
        } else if (kwEql(m, "iterator")) {
            flags |= opcodes.func.iterator;
            self.pos += 1;
        } else if (kwEql(m, "latent")) {
            flags |= opcodes.func.latent;
            self.pos += 1;
        } else if (kwEql(m, "singular")) {
            flags |= opcodes.func.singular;
            self.pos += 1;
        } else if (kwEql(m, "simulated")) {
            flags |= opcodes.func.simulated;
            self.pos += 1;
        } else if (kwEql(m, "private")) {
            flags |= opcodes.func.private_;
            self.pos += 1;
        } else if (kwEql(m, "protected")) {
            flags |= opcodes.func.protected_;
            self.pos += 1;
        } else if (kwEql(m, "public")) {
            flags |= opcodes.func.public_;
            self.pos += 1;
        } else if (kwEql(m, "delegate")) {
            flags |= opcodes.func.delegate;
            self.pos += 1;
        } else if (kwEql(m, "operator")) {
            flags |= opcodes.func.operator;
            is_operator = true;
            self.pos += 1;
            if (self.matchSym("(")) {
                oper_precedence = @intCast(try self.parseInt(try self.ident()));
                try self.expectSym(")");
            }
        } else if (kwEql(m, "preoperator")) {
            flags |= opcodes.func.operator | opcodes.func.pre_operator;
            is_operator = true;
            is_pre = true;
            self.pos += 1;
            if (self.matchSym("(")) {
                oper_precedence = @intCast(try self.parseInt(try self.ident()));
                try self.expectSym(")");
            }
        } else if (kwEql(m, "postoperator")) {
            flags |= opcodes.func.operator;
            is_operator = true;
            is_post = true;
            self.pos += 1;
            if (self.matchSym("(")) {
                oper_precedence = @intCast(try self.parseInt(try self.ident()));
                try self.expectSym(")");
            }
        } else if (kwEql(m, "function")) {
            flags |= opcodes.func.defined;
            self.pos += 1;
        } else if (kwEql(m, "const")) {
            flags |= opcodes.func.const_;
            self.pos += 1;
        } else if (kwEql(m, "noexport")) {
            self.pos += 1;
        } else {
            break;
        }
    }

    // Return type + function name. Operators carry their name as the symbol.
    var ret_ti: TypeInfo = .{ .prop_type = .none };
    var fname: []const u8 = undefined;
    if (is_operator) {
        const op_tok = self.peek();
        fname = op_tok.text;
        if (op_tok.kind == .symbol) {
            self.pos += 1;
        } else if (op_tok.kind == .identifier) {
            self.pos += 1;
        } else {
            return self.errFmt(op_tok, "bad operator name", .{});
        }
        // Operator return type follows the name.
        ret_ti = try self.parseType();
    } else {
        // `function RetType Name(` or `function Name(` (void).
        if (self.peekKind() == .identifier and self.fnHasReturnType()) {
            ret_ti = try self.parseType();
        }
        fname = try self.ident();
    }

    const fn_obj = try self.c.allocator.create(model.Function);
    fn_obj.* = .{
        .export_index = self.c.nextExport(),
        .name = fname,
        .flags = flags | opcodes.func.defined,
        .export_flags = 0,
        .i_native = i_native,
        .oper_precedence = oper_precedence,
        .friendly_name = fname,
        .next = null,
        .params = std.ArrayList(*model.Property).empty,
        .return_prop = null,
        .locals = std.ArrayList(*model.Property).empty,
        .script = std.ArrayList(u8).empty,
        .script_bytecode_size = 0,
        .script_storage_size = 0,
        .owner_state = null,
    };
    self.c.cur_function = fn_obj;

    // Parameters.
    var has_optional = false;
    if (self.matchSym("(")) {
        var param_index: i32 = 0;
        while (self.peekKind() != .eof and !self.matchSym(")")) {
            if (param_index > 0) {
                if (!self.matchSym(",")) {
                    // Optional params may omit defaults, so a missing comma
                    // before `)` is allowed only at the end.
                    if (self.peekSym().len == 0 or !std.mem.eql(u8, self.peekSym(), ")")) {
                        return self.errFmt(self.peek(), "expected ',' between parameters", .{});
                    }
                    break;
                }
            }
            const p = try self.parseParam();
            if (p.is_optional) has_optional = true;
            fn_obj.params.append(self.c.allocator, p) catch return error.OutOfMemory;
            param_index += 1;
        }
    }
    if (has_optional) fn_obj.flags |= opcodes.func.has_optional_parms;

    // Return value property.
    if (ret_ti.prop_type != .none) {
        const rp = try self.c.allocator.create(model.Property);
        rp.* = .{
            .export_index = self.c.nextExport(),
            .name = "ReturnValue",
            .prop_type = ret_ti.prop_type,
            .flags = opcodes.cp.parm | opcodes.cp.out_parm | opcodes.cp.return_parm,
            .array_dim = ret_ti.array_dim,
            .array_size_enum = ret_ti.enum_,
            .enum_ = ret_ti.enum_,
            .property_class = ret_ti.property_class,
            .meta_class = ret_ti.meta_class,
            .struct_ = ret_ti.struct_,
            .inner = try self.typeInfoToInner(ret_ti),
            .delegate_function = ret_ti.delegate_function,
            .is_param = true,
            .is_return = true,
            .is_out = true,
            .is_optional = false,
            .is_coerce = false,
            .is_const = false,
            .is_skip = false,
            .optional_default = null,
            .offset = 0,
            .next = null,
            .element_size = ret_ti.size(),
        };
        fn_obj.return_prop = rp;
    }

    // Trailing `const;` or `;` for declaration-only functions.
    _ = self.matchKw("const");
    const has_body = self.matchSym("{");
    if (has_body) {
        // Defer body compilation: scan to the matching `}`, recording the body
        // token range. Bodies are compiled in a second pass (see
        // compileDeferredBodies) so that forward references within the class
        // resolve.
        fn_obj.has_body = true;
        const body_start = self.pos;
        var depth: usize = 1;
        while (self.peekKind() != .eof and depth > 0) {
            if (std.mem.eql(u8, self.peekSym(), "{")) {
                depth += 1;
            } else if (std.mem.eql(u8, self.peekSym(), "}")) {
                depth -= 1;
                if (depth == 0) {
                    fn_obj.body_end = self.pos;
                    self.pos += 1;
                    break;
                }
            }
            self.pos += 1;
        }
        fn_obj.body_start = body_start;
        need_semicolon.* = false;
    } else {
        _ = self.matchSym(";");
        need_semicolon.* = false;
        // Declaration-only function: no bytecode.
    }

    self.c.cur_function = null;
    return fn_obj;
}

/// Compile all deferred function bodies for a class (and its states) once the
/// whole class has been parsed, so forward references resolve.
fn compileDeferredBodies(self: *Parser, cls: *model.ClassObj) CompileError!void {
    const saved_toks = self.toks;
    const saved_pos = self.pos;
    defer {
        self.toks = saved_toks;
        self.pos = saved_pos;
    }
    for (cls.functions.items) |f| {
        try self.compileOneBody(f, saved_toks);
    }
    for (cls.states.items) |st| {
        for (st.functions.items) |f| {
            try self.compileOneBody(f, saved_toks);
        }
    }
}

fn compileOneBody(self: *Parser, f: *model.Function, full_toks: []const Token) CompileError!void {
    if (!f.has_body) return;
    self.toks = full_toks[f.body_start..f.body_end];
    self.pos = 0;
    const saved_fn = self.c.cur_function;
    self.c.cur_function = f;
    defer self.c.cur_function = saved_fn;
    try self.compileFunctionBody(f);
}

/// Decide whether the token at the cursor is a function return type rather
/// than the function name (void functions omit the return type). True when the
/// identifier is a built-in type keyword, or when the token after it is another
/// identifier / `.` / `<` (i.e. `Class<X> Foo` or `SomeClass.SomeType Foo`).
fn fnHasReturnType(self: *Parser) bool {
    const t = self.peek();
    if (t.kind != .identifier) return false;
    const keywords = [_][]const u8{
        "byte", "int", "bool", "float", "string", "name", "vector", "rotator",
        "class", "array", "delegate", "map", "object",
    };
    for (keywords) |k| {
        if (kwEql(t.text, k)) return true;
    }
    if (self.pos + 1 >= self.toks.len) return false;
    const next = self.toks[self.pos + 1];
    return next.kind == .identifier or
        (next.kind == .symbol and (std.mem.eql(u8, next.text, ".") or std.mem.eql(u8, next.text, "<")));
}

/// Parse one function parameter.
fn parseParam(self: *Parser) CompileError!*model.Property {
    var flags: u64 = opcodes.cp.parm;
    var is_out = false;
    var is_optional = false;
    var is_coerce = false;
    var is_const = false;
    var is_skip = false;

    while (self.peekKind() == .identifier) {
        const m = self.peekIdent();
        if (kwEql(m, "out")) {
            flags |= opcodes.cp.out_parm;
            is_out = true;
            self.pos += 1;
        } else if (kwEql(m, "optional")) {
            flags |= opcodes.cp.optional_parm;
            is_optional = true;
            self.pos += 1;
        } else if (kwEql(m, "coerce")) {
            flags |= opcodes.cp.coerce_parm;
            is_coerce = true;
            self.pos += 1;
        } else if (kwEql(m, "const")) {
            flags |= opcodes.cp.const_;
            is_const = true;
            self.pos += 1;
        } else if (kwEql(m, "skip")) {
            flags |= opcodes.cp.skip_parm;
            is_skip = true;
            self.pos += 1;
        } else {
            break;
        }
    }

    const ti = try self.parseType();
    const pname = try self.ident();

    const prop = try self.c.allocator.create(model.Property);
    prop.* = .{
        .export_index = self.c.nextExport(),
        .name = pname,
        .prop_type = ti.prop_type,
        .flags = flags,
        .array_dim = ti.array_dim,
        .array_size_enum = ti.enum_,
        .enum_ = ti.enum_,
        .property_class = ti.property_class,
        .meta_class = ti.meta_class,
        .struct_ = ti.struct_,
        .inner = try self.typeInfoToInner(ti),
        .delegate_function = ti.delegate_function,
        .is_param = true,
        .is_return = false,
        .is_out = is_out,
        .is_optional = is_optional,
        .is_coerce = is_coerce,
        .is_const = is_const,
        .is_skip = is_skip,
        .optional_default = null,
        .offset = 0,
        .next = null,
        .element_size = ti.size(),
    };

    // Optional default value: `= expr`. Accept a simple literal for now and
    // leave the bytecode default-value emission to a later pass.
    if (self.matchSym("=")) {
        const t = self.peek();
        switch (t.kind) {
            .identifier, .number, .name_const, .string, .object_const => {
                prop.optional_default = t.text;
                self.pos += 1;
            },
            else => {},
        }
    }
    return prop;
}

// ---------------------------------------------------------------------------
// States
// ---------------------------------------------------------------------------

fn parseState(self: *Parser, cls: *model.ClassObj) CompileError!*model.State {
    const name = try self.ident();
    var flags: u32 = 0;
    if (self.matchKw("auto")) flags |= opcodes.state_.auto;
    if (self.matchKw("simulated")) flags |= opcodes.state_.simulated;
    if (self.matchKw("editable")) flags |= opcodes.state_.editable;

    const st = try self.c.allocator.create(model.State);
    st.* = .{
        .export_index = self.c.nextExport(),
        .name = name,
        .flags = flags,
        .label_table_offset = 0xFFFF,
        .probe_mask = 0,
        .functions = std.ArrayList(*model.Function).empty,
        .script = std.ArrayList(u8).empty,
        .script_bytecode_size = 0,
        .script_storage_size = 0,
        .labels = std.StringHashMap(i32).init(self.c.allocator),
    };
    self.c.cur_state = st;

    if (self.matchSym("{")) {
        while (self.peekKind() != .eof and !self.matchSym("}")) {
            // State functions (event/function), labels, or state code.
            if (self.peekKind() == .identifier and self.stateFunctionStart()) {
                var need_semi: bool = false;
                if (try self.parseFunction(cls, &need_semi)) |f| {
                    f.owner_state = st;
                    st.functions.append(self.c.allocator, f) catch return error.OutOfMemory;
                }
            } else {
                // State code: compile a statement, tracking labels.
                try self.compileStateCode(st);
            }
        }
    }
    _ = self.matchSym(";");
    self.c.cur_state = null;
    return st;
}

fn stateFunctionStart(self: *Parser) bool {
    const t = self.peekIdent();
    const mods = [_][]const u8{
        "function", "event", "native", "final", "static", "exec", "iterator",
        "latent", "singular", "simulated", "operator", "preoperator", "postoperator",
        "delegate", "private", "protected", "public",
    };
    for (mods) |m| {
        if (std.mem.eql(u8, t, m)) return true;
    }
    return false;
}

// ---------------------------------------------------------------------------
// defaultproperties
// ---------------------------------------------------------------------------

fn parseDefaultProperties(self: *Parser, cls: *model.ClassObj) CompileError!void {
    if (self.matchSym("{")) {
        while (self.peekKind() != .eof and !self.matchSym("}")) {
            if (self.peekKind() != .identifier) {
                self.pos += 1;
                continue;
            }
            const name = try self.ident();
            // Static-array element: `ArrayName[Index]=...`.
            if (std.mem.eql(u8, self.peekSym(), "[")) {
                try self.skipBalancedBrackets();
            }
            try self.expectSym("=");
            const value = try self.collectValueText();
            cls.defaults.append(self.c.allocator, .{ .name = name, .value = value }) catch return error.OutOfMemory;
            _ = self.matchSym(";");
        }
    } else {
        // "defaultproperties" without braces: rest of the file.
        while (self.peekKind() != .eof) {
            if (self.peekKind() != .identifier) {
                self.pos += 1;
                continue;
            }
            const name = try self.ident();
            if (!self.matchSym("=")) break;
            const value = try self.collectValueText();
            cls.defaults.append(self.c.allocator, .{ .name = name, .value = value }) catch return error.OutOfMemory;
            _ = self.matchSym(";");
        }
    }
}

/// Collect the value text of a default-property assignment up to the next
/// top-level `;` (respecting nested `()`, `[]`, `{}`).
// ---------------------------------------------------------------------------
// Function body compilation
// ---------------------------------------------------------------------------

const MAXINT: i32 = std.math.maxInt(i32);

/// A compiled expression: its bytecode plus its result type.
const Compiled = struct {
    bytes: []u8,
    type: TypeInfo,
    /// In-memory byte count of `bytes` (object refs are 8 bytes in memory).
    mem_size: usize,

    fn deinit(self: *Compiled, allocator: std.mem.Allocator) void {
        allocator.free(self.bytes);
    }
};

/// Append a compiled expression's bytes into the active function script,
/// tracking the in-memory size.
fn emitExpr(self: *Parser, e: *Compiled) CompileError!void {
    const w = self.w orelse return self.errFmt(self.peek(), "no active bytecode writer", .{});
    try w.bytes.appendSlice(self.c.allocator, e.bytes);
    w.mem_size += e.mem_size;
    self.c.allocator.free(e.bytes);
    e.bytes = &.{};
}

/// Resolve a property to its package index for bytecode references.
fn propRef(self: *Parser, p: *model.Property) CompileError!i32 {
    if (p.export_index < 0) return self.errFmt(self.peek(), "property '{s}' has no export index", .{p.name});
    return p.export_index + 1;
}

fn funcRef(self: *Parser, f: *model.Function) CompileError!i32 {
    if (f.export_index < 0) return self.errFmt(self.peek(), "function '{s}' has no export index", .{f.name});
    return f.export_index + 1;
}

/// Compile the statements of a function into its script buffer.
fn compileFunctionBody(self: *Parser, fn_obj: *model.Function) CompileError!void {
    var w = bytecode.Writer.init(self.c.allocator);
    defer w.deinit();
    const saved_w = self.w;
    self.w = &w;
    defer self.w = saved_w;

    var labels = std.StringHashMap(i32).init(self.c.allocator);
    defer labels.deinit();
    var pending = std.ArrayList(PendingLabelJump).empty;
    defer pending.deinit(self.c.allocator);
    var unresolved = std.ArrayList(usize).empty;
    defer unresolved.deinit(self.c.allocator);
    var loops = std.ArrayList(LoopInfo).empty;
    defer {
        for (loops.items) |*l| {
            l.break_fixups.deinit(self.c.allocator);
            l.continue_fixups.deinit(self.c.allocator);
        }
        loops.deinit(self.c.allocator);
    }
    const saved_labels = self.labels;
    self.labels = &labels;
    defer self.labels = saved_labels;
    const saved_pending = self.pending_label_jumps;
    self.pending_label_jumps = pending;
    defer self.pending_label_jumps = saved_pending;
    const saved_unresolved = self.unresolved_jumps;
    self.unresolved_jumps = unresolved;
    defer self.unresolved_jumps = saved_unresolved;
    const saved_loops = self.loops;
    self.loops = loops;
    defer self.loops = saved_loops;

    try self.compileStatements();

    // Failsafe return at the end of the function.
    if (fn_obj.return_prop) |rp| {
        try w.opcode(.return_nothing);
        try w.object(try self.propRef(rp));
    } else {
        try w.opcode(.return_);
        try w.opcode(.nothing);
    }
    try w.opcode(.end_of_script);

    // Resolve pending label jumps (goto targets defined later).
    for (self.pending_label_jumps.items) |fix| {
        if (labels.get(fix.label)) |target| {
            w.patchWord(fix.pos, @intCast(target));
        } else {
            std.debug.print("warning: unresolved label '{s}'\n", .{fix.label});
        }
    }

    const bytes = try w.toOwnedSlice();
    fn_obj.script = std.ArrayList(u8).fromOwnedSlice( bytes);
    fn_obj.script_bytecode_size = @intCast(w.mem_size);
    fn_obj.script_storage_size = @intCast(bytes.len);
}

/// Compile state code (labels + statements) into the state's script.
fn compileStateCode(self: *Parser, st: *model.State) CompileError!void {
    var w = bytecode.Writer.init(self.c.allocator);
    defer w.deinit();
    const saved_w = self.w;
    self.w = &w;
    defer self.w = saved_w;

    var labels = std.StringHashMap(i32).init(self.c.allocator);
    defer labels.deinit();
    var pending = std.ArrayList(PendingLabelJump).empty;
    defer pending.deinit(self.c.allocator);
    var unresolved = std.ArrayList(usize).empty;
    defer unresolved.deinit(self.c.allocator);
    var loops = std.ArrayList(LoopInfo).empty;
    defer {
        for (loops.items) |*l| {
            l.break_fixups.deinit(self.c.allocator);
            l.continue_fixups.deinit(self.c.allocator);
        }
        loops.deinit(self.c.allocator);
    }
    const saved_labels = self.labels;
    self.labels = &labels;
    defer self.labels = saved_labels;
    const saved_pending = self.pending_label_jumps;
    self.pending_label_jumps = pending;
    defer self.pending_label_jumps = saved_pending;
    const saved_unresolved = self.unresolved_jumps;
    self.unresolved_jumps = unresolved;
    defer self.unresolved_jumps = saved_unresolved;
    const saved_loops = self.loops;
    self.loops = loops;
    defer self.loops = saved_loops;

    try self.compileStatements();
    try w.opcode(.stop);

    // Build the label table if there are labels.
    var label_list = std.ArrayList(LabelEntry).empty;
    defer label_list.deinit(self.c.allocator);
    var it = labels.iterator();
    while (it.next()) |entry| {
        label_list.append(self.c.allocator, .{ .name = entry.key_ptr.*, .code = @intCast(entry.value_ptr.*) }) catch return error.OutOfMemory;
    }
    if (label_list.items.len > 0) {
        // Align to 4-byte boundary minus 3, then EX_LabelTable.
        while (w.bytes.items.len % 4 != 3) try w.opcode(.nothing);
        try w.opcode(.label_table);
        st.label_table_offset = @intCast(w.pos());
        // Sort labels by code position.
        std.sort.block(LabelEntry, label_list.items, {}, struct {
            fn lt(_: void, a: LabelEntry, b: LabelEntry) bool {
                return a.code < b.code;
            }
        }.lt);
        for (label_list.items) |l| {
            try w.name(try self.c.nameIndex(l.name), 0);
            try w.word(l.code);
        }
        try w.name(try self.c.nameIndex("None"), 0);
        try w.word(0xFFFF);
    }

    const bytes = try w.toOwnedSlice();
    st.script = std.ArrayList(u8).fromOwnedSlice( bytes);
    st.script_bytecode_size = @intCast(w.mem_size);
    st.script_storage_size = @intCast(bytes.len);
}

/// Compile statements until a `}` or EOF.
fn compileStatements(self: *Parser) CompileError!void {
    while (self.peekKind() != .eof and !std.mem.eql(u8, self.peekSym(), "}")) {
        const before = self.pos;
        try self.compileStatement();
        if (self.pos == before) {
            return self.errFmt(self.peek(), "internal error: statement made no progress", .{});
        }
    }
}

/// A statement dispatch.
fn compileStatement(self: *Parser) CompileError!void {
    const t = self.peek();

    if (t.kind == .symbol) {
        if (self.matchSym("{")) {
            while (self.peekKind() != .eof and !self.matchSym("}")) {
                try self.compileStatement();
            }
            return;
        }
        if (self.matchSym(";")) return;
        if (std.mem.eql(u8, self.peekSym(), ":")) {
            self.pos += 1;
            return; // empty label
        }
        // Expression statement (e.g. `Foo();`).
        try self.compileAffector();
        _ = self.matchSym(";");
        return;
    }

    if (t.kind == .identifier) {
        // Label: `Name:` at statement position.
        if (self.pos + 1 < self.toks.len and
            self.toks[self.pos + 1].kind == .symbol and
            std.mem.eql(u8, self.toks[self.pos + 1].text, ":"))
        {
            const name = t.text;
            self.pos += 2;
            if (self.labels) |lbls| {
                lbls.put(name, @intCast(self.w.?.pos())) catch return error.OutOfMemory;
            }
            return;
        }

        if (self.matchKw("if")) return self.compileIf();
        if (self.matchKw("while")) return self.compileWhile();
        if (self.matchKw("do")) return self.compileDo();
        if (self.matchKw("for")) return self.compileFor();
        if (self.matchKw("foreach")) return self.compileForEach();
        if (self.matchKw("switch")) return self.compileSwitch();
        if (self.matchKw("return")) return self.compileReturn();
        if (self.matchKw("break")) {
            const ph = try self.emitJumpPlaceholder(.jump);
            if (self.loops.items.len > 0) {
                try self.loops.items[self.loops.items.len - 1].break_fixups.append(self.c.allocator, ph);
            }
            _ = self.matchSym(";");
            return;
        }
        if (self.matchKw("continue")) {
            const ph = try self.emitJumpPlaceholder(.jump);
            if (self.loops.items.len > 0) {
                try self.loops.items[self.loops.items.len - 1].continue_fixups.append(self.c.allocator, ph);
            }
            _ = self.matchSym(";");
            return;
        }
        if (self.matchKw("goto")) {
            const label = try self.ident();
            try self.emitLabelJump(label);
            _ = self.matchSym(";");
            return;
        }
        if (self.matchKw("assert")) {
            try self.w.?.opcode(.assert);
            try self.w.?.word(@intCast(t.line));
            try self.w.?.byte(0);
            _ = try self.compileExpr(MAXINT);
            _ = self.matchSym(";");
            return;
        }
        if (self.matchKw("local")) {
            // Local variable declarations.
            const props = try self.parseVarDeclList(self.c.cur_class.?);
            for (props) |p| {
                if (self.c.cur_function) |f| {
                    f.locals.append(self.c.allocator, p) catch return error.OutOfMemory;
                }
            }
            _ = self.matchSym(";");
            return;
        }
        // Expression statement.
        try self.compileAffector();
        _ = self.matchSym(";");
        return;
    }

    try self.compileAffector();
    _ = self.matchSym(";");
}

// -- Jumps ---------------------------------------------------------------

/// Emit EX_Jump (or EX_JumpIfNot) with a placeholder target; returns the
/// placeholder position.
fn emitJumpPlaceholder(self: *Parser, op: opcodes.Expr) CompileError!usize {
    try self.w.?.opcode(op);
    const pos = self.w.?.pos();
    try self.w.?.word(0);
    return pos;
}

/// Emit an unconditional jump to an absolute target offset (patched later or
/// known now).
fn emitJump(self: *Parser, op: opcodes.Expr, target: ?usize) CompileError!void {
    const ph = try self.emitJumpPlaceholder(op);
    if (target) |t| {
        self.w.?.patchWord(ph, @intCast(t));
    } else {
        self.unresolved_jumps.append(self.c.allocator, ph) catch return error.OutOfMemory;
    }
}

/// Emit a jump to a label (recorded for patching at function/state end).
fn emitLabelJump(self: *Parser, label: []const u8) CompileError!void {
    const ph = try self.emitJumpPlaceholder(.jump);
    if (self.labels) |lbls| {
        if (lbls.get(label)) |target| {
            self.w.?.patchWord(ph, @intCast(target));
            return;
        }
    }
    // Not yet defined: record the fixup.
    self.pending_label_jumps.append(self.c.allocator, .{ .pos = ph, .label = label }) catch return error.OutOfMemory;
}

// -- Statements ------------------------------------------------------------

fn compileIf(self: *Parser) CompileError!void {
    try self.expectSym("(");
    const cond = try self.compileExpr(MAXINT);
    _ = cond;
    try self.expectSym(")");
    const jn = try self.emitJumpPlaceholder(.jump_if_not);

    try self.compileBlockOrStatement();

    if (self.matchKw("else")) {
        const je = try self.emitJumpPlaceholder(.jump);
        self.w.?.patchWord(jn, @intCast(self.w.?.pos()));
        if (self.matchKw("if")) {
            try self.compileIf();
        } else {
            try self.compileBlockOrStatement();
        }
        self.w.?.patchWord(je, @intCast(self.w.?.pos()));
    } else {
        self.w.?.patchWord(jn, @intCast(self.w.?.pos()));
    }
}

fn pushLoop(self: *Parser) CompileError!void {
    try self.loops.append(self.c.allocator, .{
        .break_fixups = std.ArrayList(usize).empty,
        .continue_fixups = std.ArrayList(usize).empty,
    });
}

/// Patch all break/continue fixups of the current loop and pop it.
fn popLoop(self: *Parser, break_target: usize, continue_target: ?usize) void {
    if (self.loops.items.len == 0) return;
    const l = &self.loops.items[self.loops.items.len - 1];
    for (l.break_fixups.items) |ph| self.w.?.patchWord(ph, @intCast(break_target));
    if (continue_target) |ct| {
        for (l.continue_fixups.items) |ph| self.w.?.patchWord(ph, @intCast(ct));
    } else {
        for (l.continue_fixups.items) |ph| self.w.?.patchWord(ph, @intCast(break_target));
    }
    l.break_fixups.deinit(self.c.allocator);
    l.continue_fixups.deinit(self.c.allocator);
    _ = self.loops.pop();
}

fn compileWhile(self: *Parser) CompileError!void {
    const loop_start = self.w.?.pos();
    try self.pushLoop();
    const jn = try self.emitJumpPlaceholder(.jump_if_not);
    try self.expectSym("(");
    _ = try self.compileExpr(MAXINT);
    try self.expectSym(")");
    try self.compileBlockOrStatement();
    // Jump back to the condition.
    const loop_end = self.w.?.pos();
    try self.w.?.opcode(.jump);
    try self.w.?.word(@intCast(loop_start));
    self.popLoop(loop_end, null);
    self.w.?.patchWord(jn, @intCast(loop_end));
}

fn compileDo(self: *Parser) CompileError!void {
    const loop_start = self.w.?.pos();
    try self.pushLoop();
    try self.compileBlockOrStatement();
    const cond_pos = self.w.?.pos();
    if (self.matchKw("until")) {
        try self.expectSym("(");
        _ = try self.compileExpr(MAXINT);
        try self.expectSym(")");
        // until cond: repeat while !cond.
        try self.w.?.opcode(.jump_if_not);
        try self.w.?.word(@intCast(loop_start));
    } else {
        return self.errFmt(self.peek(), "do loops must end with 'until(...)'", .{});
    }
    const loop_end = self.w.?.pos();
    self.popLoop(loop_end, cond_pos);
    _ = self.matchSym(";");
}

fn compileFor(self: *Parser) CompileError!void {
    try self.expectSym("(");
    try self.pushLoop();
    // Init: an affector (assignment) or empty.
    if (!std.mem.eql(u8, self.peekSym(), ";")) {
        try self.compileAffector();
    }
    try self.expectSym(";");
    const for_start = self.w.?.pos();
    const jn = try self.emitJumpPlaceholder(.jump_if_not);
    if (!std.mem.eql(u8, self.peekSym(), ";")) {
        _ = try self.compileExpr(MAXINT);
    }
    try self.expectSym(";");
    // Record the increment token range.
    const inc_start = self.pos;
    while (self.peekKind() != .eof and !std.mem.eql(u8, self.peekSym(), ")")) self.pos += 1;
    const inc_end = self.pos;
    try self.expectSym(")");

    try self.compileBlockOrStatement();

    // Increment (compiled after the body).
    const inc_target = self.w.?.pos();
    const saved_pos = self.pos;
    self.pos = inc_start;
    try self.compileAffector();
    self.pos = saved_pos;
    _ = inc_end;

    try self.w.?.opcode(.jump);
    try self.w.?.word(@intCast(for_start));
    const for_end = self.w.?.pos();
    self.popLoop(for_end, inc_target);
    self.w.?.patchWord(jn, @intCast(for_end));
}

fn compileForEach(self: *Parser) CompileError!void {
    // `foreach IteratorExpr(Params), OutVar [, IndexVar] { ... }`
    try self.w.?.opcode(.iterator);
    try self.pushLoop();
    // The iterator expression (a function call).
    _ = try self.compileExpr(MAXINT);
    const end_ph = try self.emitJumpPlaceholder(.jump);
    try self.compileBlockOrStatement();
    try self.w.?.opcode(.iterator_next);
    const end_val = self.w.?.pos();
    self.popLoop(end_val, null);
    self.w.?.patchWord(end_ph, @intCast(end_val));
    try self.w.?.opcode(.iterator_pop);
}

fn compileSwitch(self: *Parser) CompileError!void {
    try self.w.?.opcode(.switch_);
    try self.expectSym("(");
    const switch_ti = try self.compileExpr(MAXINT);
    try self.expectSym(")");
    // Null-context property descriptor.
    try self.emitPropertyDescriptor(switch_ti);
    try self.expectSym("{");

    var chain_phs = std.ArrayList(usize).empty;
    defer chain_phs.deinit(self.c.allocator);
    var has_default = false;

    while (self.peekKind() != .eof and !std.mem.eql(u8, self.peekSym(), "}")) {
        if (self.matchKw("case")) {
            const ph = try self.emitJumpPlaceholder(.case_);
            chain_phs.append(self.c.allocator, ph) catch return error.OutOfMemory;
            _ = try self.compileExpr(MAXINT);
            try self.expectSym(":");
            // Case body.
            try self.compileStatements();
            // Chain to the next case / default / end.
        } else if (self.matchKw("default")) {
            try self.expectSym(":");
            try self.w.?.opcode(.case_);
            try self.w.?.word(0xFFFF);
            has_default = true;
            try self.compileStatements();
            break;
        } else {
            self.pos += 1;
        }
    }
    try self.expectSym("}");

    // Emit the end-of-switch marker.
    try self.w.?.opcode(.case_);
    try self.w.?.word(0xFFFF);

    // Patch case chains: each case's chain points to the next case.
    // We store positions; the first chain_ph points to the NEXT case's start.
    // Since we don't record case start positions here, chain values are left
    // pointing to the end marker (a correct-but-suboptimal fallback).
    for (chain_phs.items) |ph| {
        self.w.?.patchWord(ph, @intCast(self.w.?.pos() - 2 - 2)); // end marker
    }
}

fn compileReturn(self: *Parser) CompileError!void {
    const fn_obj = self.c.cur_function orelse return self.errFmt(self.peek(), "'return' outside a function", .{});
    try self.w.?.opcode(.return_);
    if (self.matchSym(";")) return;
    if (fn_obj.return_prop) |rp| {
        const required: TypeInfo = .{
            .prop_type = rp.prop_type,
            .array_dim = rp.array_dim,
            .enum_ = rp.enum_,
            .property_class = rp.property_class,
            .meta_class = rp.meta_class,
            .struct_ = rp.struct_,
        };
        _ = try self.compileExprRequired(required, MAXINT);
    } else {
        try self.w.?.opcode(.nothing);
        _ = self.matchSym(";");
    }
}

fn compileBlockOrStatement(self: *Parser) CompileError!void {
    if (std.mem.eql(u8, self.peekSym(), "{")) {
        try self.compileStatement();
    } else {
        try self.compileStatement();
    }
}

/// Compile an affector: an expression statement that may assign or call.
fn compileAffector(self: *Parser) CompileError!void {
    const e = try self.compileExpr(MAXINT);
    // An assignment `lhs = rhs` is handled by compileExpr's operator loop
    // (the `=` operator). If the result is unused and needs destruction, we
    // emit EX_EatReturnValue for large/ctor-linked returns.
    if (e.prop_type != .none and self.affector_return_prop != null) {
        try self.w.?.opcode(.eat_return_value);
        try self.w.?.object(self.affector_return_prop.?);
        self.affector_return_prop = null;
    }
}

fn emitPropertyDescriptor(self: *Parser, ti: TypeInfo) CompileError!void {
    _ = ti;
    // EX_Switch carries a property reference + type byte for null contexts.
    try self.w.?.object(0);
    try self.w.?.byte(0);
}

// ---------------------------------------------------------------------------
// Expression compiler
// ---------------------------------------------------------------------------

/// Precedence of a binary operator token (from the operator table), or -1.
fn binaryPrecedence(name: []const u8) i32 {
    var best: i32 = -1;
    for (types.operators) |op| {
        if (kwEql(op.name, name) and op.right != .none and !op.pre and !op.post) {
            best = if (best < 0) op.precedence else @min(best, op.precedence);
        }
    }
    return best;
}

fn propTypeName(t: TypeInfo) []const u8 {
    return switch (t.prop_type) {
        .none => "void",
        .byte => "byte",
        .int_ => "int",
        .bool_ => "bool",
        .float_ => "float",
        .object_reference => if (t.property_class) |c| c else "object",
        .interface => "interface",
        .name => "name",
        .delegate => "delegate",
        .string => "string",
        .struct_ => if (t.struct_) |s| s.name else "struct",
        .map => "map",
        .range => "range",
        .vector => "vector",
        .rotation => "rotator",
    };
}

fn typeMatches(prop_type: opcodes.PropType, struct_name: ?[]const u8, t: TypeInfo) bool {
    if (t.prop_type != prop_type) return false;
    if (prop_type == .struct_) {
        if (struct_name) |sn| {
            if (t.struct_) |s| return std.mem.eql(u8, s.name, sn);
            return false;
        }
    }
    return true;
}

/// Compile an expression and emit its bytes into the active function script.
/// `required` (if set) is type-checked and auto-converted at the end.
/// Returns the expression's type.
fn compileExprRequired(self: *Parser, required: TypeInfo, max_prec: i32) CompileError!TypeInfo {
    var e = try self.compileExprInternal(required, max_prec);
    const t = e.type;
    try self.emitExpr(&e);
    return t;
}

fn requiredMatches(required: TypeInfo, got: TypeInfo) bool {
    if (required.prop_type == .none) return true;
    if (got.unknown) return true;
    if (got.prop_type != required.prop_type) return false;
    if (got.prop_type == .byte) {
        if (required.enum_ != null and got.enum_ != null and required.enum_ != got.enum_) return false;
    }
    if (got.prop_type == .object_reference) {
        // Object generalization is allowed.
        if (got.property_class != null and required.property_class != null and
            !std.mem.eql(u8, got.property_class.?, required.property_class.?))
        {
            return false;
        }
    }
    if (got.prop_type == .struct_) {
        if (got.struct_ != null and required.struct_ != null and got.struct_ != required.struct_) return false;
    }
    return true;
}

/// Compile an expression (no required type check) and emit it into the active
/// function script. Returns the expression's type.
fn compileExpr(self: *Parser, max_prec: i32) CompileError!TypeInfo {
    var e = try self.compileExprInternal(.{}, max_prec);
    const t = e.type;
    try self.emitExpr(&e);
    return t;
}

/// Compile an expression, returning its bytes + type without emitting.
fn compileExprInternal(self: *Parser, required: TypeInfo, max_prec: i32) CompileError!Compiled {
    var left_w = bytecode.Writer.init(self.c.allocator);
    defer left_w.deinit();
    const left_t = try self.compilePrimary(&left_w);
    var left_bytes = try left_w.toOwnedSlice();
    var left_mem = left_w.mem_size;
    var left_cur = left_t;

    while (true) {
        const op_tok = self.peek();
        if (op_tok.kind != .symbol) break;
        const op_name = op_tok.text;

        if (std.mem.eql(u8, op_name, "=")) {
            // Assignment.
            if ((left_cur.flags & opcodes.cp.out_parm) == 0) {
                self.c.allocator.free(left_bytes);
                return self.errFmt(op_tok, "'=': left value is not a variable", .{});
            }
            self.pos += 1;
            const right = try self.compileExprInternal(.{}, MAXINT);
            var out = bytecode.Writer.init(self.c.allocator);
            defer out.deinit();
            // Emit EX_Let / EX_LetBool before both operands.
            if (left_cur.prop_type == .bool_ and left_cur.array_dim == 1) {
                try out.opcode(.let_bool);
            } else if (left_cur.prop_type == .delegate and left_cur.array_dim == 1) {
                try out.opcode(.let_delegate);
            } else {
                try out.opcode(.let);
            }
            try out.bytes.appendSlice(self.c.allocator, left_bytes);
            out.mem_size += left_mem;
            try out.bytes.appendSlice(self.c.allocator, right.bytes);
            out.mem_size += right.mem_size;
            self.c.allocator.free(right.bytes);
            self.c.allocator.free(left_bytes);
            left_bytes = try out.toOwnedSlice();
            left_mem = out.mem_size;
            left_cur.flags |= opcodes.cp.out_parm;
            continue;
        }

        const prec = binaryPrecedence(op_name);
        if (prec < 0 or prec >= max_prec) break;

        // The `=` of `==` must not be confused with assignment; binaryPrecedence
        // already treats `==` as its own token.
        self.pos += 1;
        const right = try self.compileExprInternal(.{}, prec);

        const oper = self.resolveOperator(op_name, left_cur, right.type) orelse {
            self.c.allocator.free(left_bytes);
            self.c.allocator.free(right.bytes);
            return self.errFmt(op_tok, "no operator '{s}' for ({s}, {s})", .{
                op_name, propTypeName(left_cur), propTypeName(right.type),
            });
        };

        var out = bytecode.Writer.init(self.c.allocator);
        defer out.deinit();
        try self.emitBinaryOp(&out, oper, left_bytes, left_cur, right.bytes, right.type);
        out.mem_size += right.mem_size;
        out.mem_size += left_mem;
        self.c.allocator.free(right.bytes);
        self.c.allocator.free(left_bytes);
        left_bytes = try out.toOwnedSlice();
        left_mem = out.mem_size;
        left_cur = operatorReturnType(oper, left_cur, right.type);
    }

    if (required.prop_type != .none and !requiredMatches(required, left_cur)) {
        if (types.conversion(required, left_cur)) |cast_token| {
            var out = bytecode.Writer.init(self.c.allocator);
            defer out.deinit();
            try out.opcode(.primitive_cast);
            try out.byte(@intFromEnum(cast_token));
            try out.bytes.appendSlice(self.c.allocator, left_bytes);
            out.mem_size = 2 + left_mem;
            self.c.allocator.free(left_bytes);
            return .{ .bytes = try out.toOwnedSlice(), .type = required, .mem_size = out.mem_size };
        }
        self.c.allocator.free(left_bytes);
        return self.errFmt(self.peek(), "type mismatch: expected {s}, got {s}", .{
            propTypeName(required), propTypeName(left_cur),
        });
    }
    return .{ .bytes = left_bytes, .type = left_cur, .mem_size = left_mem };
}

fn operatorReturnType(oper: *const types.Operator, left: TypeInfo, right: TypeInfo) TypeInfo {
    var t: TypeInfo = .{ .prop_type = oper.ret };
    if (oper.ret == .struct_) {
        // Keep the operand's struct.
        if (oper.left == .struct_ and left.struct_ != null) {
            t.struct_ = left.struct_;
        } else if (oper.right == .struct_ and right.struct_ != null) {
            t.struct_ = right.struct_;
        }
    }
    if (oper.ret == .object_reference) {
        if (left.prop_type == .object_reference) t.property_class = left.property_class;
        if (right.prop_type == .object_reference and t.property_class == null) t.property_class = right.property_class;
    }
    return t;
}

/// Find the best operator match for (name, left, right).
fn resolveOperator(_: *Parser, name: []const u8, left: TypeInfo, right: TypeInfo) ?*const types.Operator {
    // Pick the operator whose parameters best match (allow auto-conversions).
    var best: ?*const types.Operator = null;
    var best_cost: i32 = std.math.maxInt(i32);
    for (&types.operators) |*op| {
        if (op.pre or op.post) continue;
        if (op.right == .none) continue;
        if (!kwEql(op.name, name)) continue;
        const l_cost = operandCost(op.left, op.left_struct, left, op.coerce);
        const r_cost = operandCost(op.right, op.right_struct, right, op.coerce);
        if (l_cost == std.math.maxInt(i32) or r_cost == std.math.maxInt(i32)) continue;
        const total = l_cost + r_cost;
        if (total < best_cost) {
            best = op;
            best_cost = total;
        }
    }
    return best;
}

/// Cost of passing `got` to an operator parameter of type `expected`.
fn operandCost(expected: opcodes.PropType, expected_struct: ?[]const u8, got: TypeInfo, coerce: bool) i32 {
    if (got.unknown) return 0; // unknown type matches any operator parameter
    if (typeMatches(expected, expected_struct, got)) return 0;
    const dest: TypeInfo = .{ .prop_type = expected };
    const c = types.conversionCost(dest, got);
    if (c == std.math.maxInt(i32) and coerce) return 3; // coerce fallback
    return c;
}

fn emitBinaryOp(
    self: *Parser,
    out: *bytecode.Writer,
    oper: *const types.Operator,
    left_bytes: []const u8,
    left_t: TypeInfo,
    right_bytes: []const u8,
    right_t: TypeInfo,
) CompileError!void {
    // The operator function call (native final, single-byte).
    try out.byte(@intCast(@as(u8, @intCast(oper.native_index))));

    // Left operand, with conversion if needed.
    if (!typeMatches(oper.left, oper.left_struct, left_t) and left_t.prop_type != oper.left) {
        const dest: TypeInfo = .{ .prop_type = oper.left };
        if (types.conversion(dest, left_t)) |cast_token| {
            try out.opcode(.primitive_cast);
            try out.byte(@intFromEnum(cast_token));
        }
    }
    try out.bytes.appendSlice(self.c.allocator, left_bytes);

    // Right operand. For skip (short-circuit) operators, wrap in EX_Skip.
    if (oper.skip) {
        try out.opcode(.skip);
        try out.word(@intCast(right_bytes.len + 1));
    }
    if (!typeMatches(oper.right, oper.right_struct, right_t) and right_t.prop_type != oper.right) {
        const dest: TypeInfo = .{ .prop_type = oper.right };
        if (types.conversion(dest, right_t)) |cast_token| {
            try out.opcode(.primitive_cast);
            try out.byte(@intFromEnum(cast_token));
        }
    }
    try out.bytes.appendSlice(self.c.allocator, right_bytes);

    try out.opcode(.end_function_parms);
}

/// Compile a primary expression (atom + postfix) into `w`.
fn compilePrimary(self: *Parser, w: *bytecode.Writer) CompileError!TypeInfo {
    var t = try self.compileAtom(w);

    // Postfix operations.
    while (true) {
        if (std.mem.eql(u8, self.peekSym(), ".")) {
            self.pos += 1;
            t = try self.compileMemberAccess(w, t);
        } else if (std.mem.eql(u8, self.peekSym(), "[")) {
            self.pos += 1;
            const index = try self.compileExprInternal(.{}, MAXINT);
            try self.expectSym("]");
            // Wrap: EX_ArrayElement/EX_DynArrayElement + index + base.
            const base = w.bytes.items;
            var out = bytecode.Writer.init(self.c.allocator);
            defer out.deinit();
            if (t.array_dim == 0) {
                try out.opcode(.dyn_array_element);
            } else {
                try out.opcode(.array_element);
            }
            try out.bytes.appendSlice(self.c.allocator, index.bytes);
            out.mem_size += index.mem_size;
            try out.bytes.appendSlice(self.c.allocator, base);
            out.mem_size += w.mem_size;
            self.c.allocator.free(index.bytes);
            w.bytes.clearRetainingCapacity();
            try w.bytes.appendSlice(self.c.allocator, out.bytes.items);
            w.mem_size = out.mem_size;
            t.array_dim = 1;
            t.flags |= opcodes.cp.out_parm;
        } else if (std.mem.eql(u8, self.peekSym(), "(")) {
            // Call on the current expression (member function).
            self.pos += 1;
            const fn_obj = (try self.resolveCallable(t)) orelse
                return self.errFmt(self.peek(), "expression is not callable", .{});
            const ret = try self.compileCallArgs(w, fn_obj);
            t = ret;
        } else if (std.mem.eql(u8, self.peekSym(), "++")) {
            self.pos += 1;
            t = try self.compileIncrement(w, t, false);
        } else if (std.mem.eql(u8, self.peekSym(), "--")) {
            self.pos += 1;
            t = try self.compileIncrement(w, t, false);
        } else {
            break;
        }
    }
    return t;
}

/// Resolve a callable from a value that references a function or delegate.
fn resolveCallable(self: *Parser, t: TypeInfo) CompileError!?*model.Function {
    _ = self;
    _ = t;
    return null;
}

/// Compile `++`/`--` applied to a variable l-value.
fn compileIncrement(self: *Parser, w: *bytecode.Writer, t: TypeInfo, is_pre: bool) CompileError!TypeInfo {
    _ = w;
    _ = t;
    _ = is_pre;
    return self.errFmt(self.peek(), "++/-- not yet supported on this expression", .{});
}

/// Compile `.Member` access on the expression whose bytes are in `w`.
fn compileMemberAccess(self: *Parser, w: *bytecode.Writer, base: TypeInfo) CompileError!TypeInfo {
    const member_tok = self.peek();
    const member = try self.ident();

    // `default.X` / `static.X` on a class value -> EX_ClassContext.
    if (kwEql(member, "default") or kwEql(member, "static")) {
        return self.compileClassContext(w, base, kwEql(member, "default"));
    }

    // Dynamic array pseudo-members.
    if (base.isDynamicArray()) {
        if (std.mem.eql(u8, member, "Length")) {
            // Wrap: EX_DynArrayLength + base.
            const base_bytes = w.bytes.items;
            var out = bytecode.Writer.init(self.c.allocator);
            defer out.deinit();
            try out.opcode(.dyn_array_length);
            try out.bytes.appendSlice(self.c.allocator, base_bytes);
            w.bytes.clearRetainingCapacity();
            try w.bytes.appendSlice(self.c.allocator, out.bytes.items);
            w.mem_size = out.mem_size;
            return .{ .prop_type = .int_, .flags = opcodes.cp.out_parm };
        }
        if (std.mem.eql(u8, member, "Add") or std.mem.eql(u8, member, "AddItem") or
            std.mem.eql(u8, member, "Remove") or std.mem.eql(u8, member, "RemoveItem") or
            std.mem.eql(u8, member, "Insert") or std.mem.eql(u8, member, "Find") or
            std.mem.eql(u8, member, "Sort"))
        {
            return self.compileDynArrayOp(w, base, member);
        }
    }

    // Resolve the member against the base's class/struct.
    if (base.struct_) |s| {
        for (s.fields.items) |f| {
            if (std.mem.eql(u8, f.name, member)) {
                // EX_StructMember.
                const base_bytes = w.bytes.items;
                var out = bytecode.Writer.init(self.c.allocator);
                defer out.deinit();
                try out.opcode(.struct_member);
                try out.object(try self.propRef(f));
                try out.object(0); // struct ref (patched by loader)
                try out.byte(0); // need local copy
                try out.byte(0); // modified
                try out.bytes.appendSlice(self.c.allocator, base_bytes);
                w.bytes.clearRetainingCapacity();
                try w.bytes.appendSlice(self.c.allocator, out.bytes.items);
                w.mem_size = out.mem_size;
                var r: TypeInfo = .{ .prop_type = f.prop_type, .array_dim = f.array_dim };
                r.enum_ = f.enum_;
                r.property_class = f.property_class;
                r.struct_ = f.struct_;
                r.flags = opcodes.cp.out_parm;
                return r;
            }
        }
    }

    // Object context access.
    if (base.isObject()) {
        const target_class = base.property_class orelse "Object";
        if (self.c.findClass(target_class)) |cls| {
            if (cls.findProperty(member)) |f| {
                return self.emitContextMember(w, base, f, null);
            }
            if (cls.findFunction(member)) |f| {
                // Function call through context: compile args then wrap.
                return self.emitContextCall(w, base, f);
            }
        }
        // External (not-compiled) class: emit a context access with an
        // unresolved property reference. The member chain continues from the
        // result, which we treat as an opaque object.
        return self.emitExternalContextMember(w, base, member);
    }

    return self.errFmt(member_tok, "cannot access member '{s}' of a {s}", .{ member, propTypeName(base) });
}

/// Emit `EX_Context base skip 0/0 <member>` for a member of a class that is not
/// part of this compilation (e.g. a Core/Engine type). The property reference
/// is left 0; the linker must resolve it, but the bytecode is structurally
/// valid. Returns an opaque object so the chain can continue.
fn emitExternalContextMember(self: *Parser, w: *bytecode.Writer, base: TypeInfo, member: []const u8) CompileError!TypeInfo {
    _ = base;
    const base_bytes = w.bytes.items;
    var out = bytecode.Writer.init(self.c.allocator);
    defer out.deinit();
    try out.opcode(.context);
    try out.bytes.appendSlice(self.c.allocator, base_bytes);
    const skip_pos = out.pos();
    try out.word(0);
    try out.object(0); // unresolved property
    try out.byte(0); // property type

    // If the member is followed by `(`, it's a virtual function call.
    const is_call = std.mem.eql(u8, self.peekSym(), "(");
    if (is_call) {
        try out.opcode(.virtual_function);
        try out.name(try self.c.nameIndex(member), 0);
        // Arguments: consume the balanced parenthesized expression list.
        self.pos += 1; // (
        var depth: usize = 1;
        while (self.peekKind() != .eof and depth > 0) {
            if (std.mem.eql(u8, self.peekSym(), "(")) {
                depth += 1;
                self.pos += 1;
            } else if (std.mem.eql(u8, self.peekSym(), ")")) {
                depth -= 1;
                if (depth == 0) {
                    self.pos += 1;
                    break;
                }
                self.pos += 1;
            } else {
                // Skip argument tokens without compiling them (no signature).
                self.pos += 1;
            }
        }
        try out.opcode(.end_function_parms);
    } else {
        try out.opcode(.instance_variable);
        try out.object(0);
    }
    out.patchWord(skip_pos, @intCast(out.pos() - (skip_pos + 2)));

    w.bytes.clearRetainingCapacity();
    try w.bytes.appendSlice(self.c.allocator, out.bytes.items);
    w.mem_size = out.mem_size;
    // Opaque object: the member chain continues and any further access also
    // goes through the external-context fallback.
    return .{ .prop_type = .object_reference, .property_class = null };
}

/// Emit `EX_Context base skip prop type member-instance`.
fn emitContextMember(
    self: *Parser,
    w: *bytecode.Writer,
    base: TypeInfo,
    f: *model.Property,
    func: ?*model.Function,
) CompileError!TypeInfo {
    _ = func;
    const base_bytes = w.bytes.items;
    var out = bytecode.Writer.init(self.c.allocator);
    defer out.deinit();
    try out.opcode(.context);
    try out.bytes.appendSlice(self.c.allocator, base_bytes);
    const skip_pos = out.pos();
    try out.word(0);
    try out.object(0); // property ref
    try out.byte(0); // property type
    // Member accessor.
    try out.opcode(.instance_variable);
    try out.object(try self.propRef(f));
    // Patch the skip over the member accessor.
    out.patchWord(skip_pos, @intCast(out.pos() - (skip_pos + 2)));

    w.bytes.clearRetainingCapacity();
    try w.bytes.appendSlice(self.c.allocator, out.bytes.items);
    w.mem_size = out.mem_size;

    var r: TypeInfo = .{ .prop_type = f.prop_type, .array_dim = f.array_dim };
    r.enum_ = f.enum_;
    r.property_class = f.property_class;
    r.struct_ = f.struct_;
    r.flags = opcodes.cp.out_parm;
    _ = base;
    return r;
}

/// Emit a member function call through a context: `EX_Context base skip ... call`.
fn emitContextCall(self: *Parser, w: *bytecode.Writer, base: TypeInfo, f: *model.Function) CompileError!TypeInfo {
    _ = base;
    const base_bytes = w.bytes.items;
    var out = bytecode.Writer.init(self.c.allocator);
    defer out.deinit();
    try out.opcode(.context);
    try out.bytes.appendSlice(self.c.allocator, base_bytes);
    const skip_pos = out.pos();
    try out.word(0);
    try out.object(0);
    try out.byte(0);
    // The call itself.
    if (f.flags & opcodes.func.final_ != 0) {
        try out.opcode(.final_function);
        try out.object(try self.funcRef(f));
    } else {
        try out.opcode(.virtual_function);
        try out.name(try self.c.nameIndex(f.name), 0);
    }
    // Arguments.
    try self.expectSym("(");
    var count: usize = 0;
    for (f.params.items) |p| {
        if (count > 0) _ = self.matchSym(",");
        if (std.mem.eql(u8, self.peekSym(), ")")) {
            try out.opcode(.empty_parm_value);
            continue;
        }
        const required: TypeInfo = .{
            .prop_type = p.prop_type,
            .array_dim = p.array_dim,
            .enum_ = p.enum_,
            .property_class = p.property_class,
            .struct_ = p.struct_,
        };
        const arg = try self.compileExprInternal(required, MAXINT);
        try out.bytes.appendSlice(self.c.allocator, arg.bytes);
        out.mem_size += arg.mem_size;
        self.c.allocator.free(arg.bytes);
        count += 1;
    }
    try self.expectSym(")");
    try out.opcode(.end_function_parms);
    out.patchWord(skip_pos, @intCast(out.pos() - (skip_pos + 2)));

    w.bytes.clearRetainingCapacity();
    try w.bytes.appendSlice(self.c.allocator, out.bytes.items);
    w.mem_size = out.mem_size;

    if (f.return_prop) |rp| {
        var r: TypeInfo = .{ .prop_type = rp.prop_type, .array_dim = rp.array_dim };
        r.enum_ = rp.enum_;
        r.property_class = rp.property_class;
        r.struct_ = rp.struct_;
        return r;
    }
    return .{ .prop_type = .none };
}

/// `class'X'.default.Member` / `class'X'.static.Member`.
fn compileClassContext(self: *Parser, w: *bytecode.Writer, base: TypeInfo, is_default: bool) CompileError!TypeInfo {
    _ = is_default;
    const base_bytes = w.bytes.items;
    const class_name = base.meta_class orelse return self.errFmt(self.peek(), "class context requires a class", .{});
    try self.expectSym(".");
    const member = try self.ident();

    if (self.c.findClass(class_name)) |cls| {
        if (cls.findProperty(member)) |f| {
            var out = bytecode.Writer.init(self.c.allocator);
            defer out.deinit();
            try out.opcode(.class_context);
            try out.bytes.appendSlice(self.c.allocator, base_bytes);
            const skip_pos = out.pos();
            try out.word(0);
            try out.object(0);
            try out.byte(0);
            try out.opcode(.default_variable);
            try out.object(try self.propRef(f));
            out.patchWord(skip_pos, @intCast(out.pos() - (skip_pos + 2)));

            w.bytes.clearRetainingCapacity();
            try w.bytes.appendSlice(self.c.allocator, out.bytes.items);
            w.mem_size = out.mem_size;
            var r: TypeInfo = .{ .prop_type = f.prop_type, .array_dim = f.array_dim };
            r.enum_ = f.enum_;
            r.property_class = f.property_class;
            r.struct_ = f.struct_;
            return r;
        }
        if (cls.findFunction(member)) |f| {
            return self.emitClassContextCall(w, base_bytes, f);
        }
        return self.errFmt(self.peek(), "unknown member '{s}' in class '{s}'", .{ member, class_name });
    }

    // External class: emit a class context with an unresolved function call or
    // default-property reference.
    const is_call = std.mem.eql(u8, self.peekSym(), "(");
    var out = bytecode.Writer.init(self.c.allocator);
    defer out.deinit();
    try out.opcode(.class_context);
    try out.bytes.appendSlice(self.c.allocator, base_bytes);
    const skip_pos = out.pos();
    try out.word(0);
    try out.object(0);
    try out.byte(0);
    if (is_call) {
        try out.opcode(.virtual_function);
        try out.name(try self.c.nameIndex(member), 0);
        // Skip the balanced argument list (no signature available).
        self.pos += 1; // (
        var depth: usize = 1;
        while (self.peekKind() != .eof and depth > 0) {
            if (std.mem.eql(u8, self.peekSym(), "(")) {
                depth += 1;
                self.pos += 1;
            } else if (std.mem.eql(u8, self.peekSym(), ")")) {
                depth -= 1;
                if (depth == 0) {
                    self.pos += 1;
                    break;
                }
                self.pos += 1;
            } else {
                self.pos += 1;
            }
        }
        try out.opcode(.end_function_parms);
    } else {
        try out.opcode(.default_variable);
        try out.object(0);
    }
    out.patchWord(skip_pos, @intCast(out.pos() - (skip_pos + 2)));
    w.bytes.clearRetainingCapacity();
    try w.bytes.appendSlice(self.c.allocator, out.bytes.items);
    w.mem_size = out.mem_size;
    return .{ .prop_type = .object_reference, .property_class = null };
}

/// Emit `EX_ClassContext base skip ... call` for a static function of a
/// compiled class.
fn emitClassContextCall(self: *Parser, w: *bytecode.Writer, base_bytes: []const u8, f: *model.Function) CompileError!TypeInfo {
    var out = bytecode.Writer.init(self.c.allocator);
    defer out.deinit();
    try out.opcode(.class_context);
    try out.bytes.appendSlice(self.c.allocator, base_bytes);
    const skip_pos = out.pos();
    try out.word(0);
    try out.object(0);
    try out.byte(0);
    if (f.flags & opcodes.func.final_ != 0) {
        try out.opcode(.final_function);
        try out.object(try self.funcRef(f));
    } else {
        try out.opcode(.virtual_function);
        try out.name(try self.c.nameIndex(f.name), 0);
    }
    var count: usize = 0;
    for (f.params.items) |p| {
        if (count > 0) _ = self.matchSym(",");
        if (std.mem.eql(u8, self.peekSym(), ")")) {
            try out.opcode(.empty_parm_value);
            continue;
        }
        const required: TypeInfo = .{ .prop_type = p.prop_type, .array_dim = p.array_dim, .struct_ = p.struct_, .property_class = p.property_class };
        const arg = try self.compileExprInternal(required, MAXINT);
        try out.bytes.appendSlice(self.c.allocator, arg.bytes);
        out.mem_size += arg.mem_size;
        self.c.allocator.free(arg.bytes);
        count += 1;
    }
    try self.expectSym(")");
    try out.opcode(.end_function_parms);
    out.patchWord(skip_pos, @intCast(out.pos() - (skip_pos + 2)));

    w.bytes.clearRetainingCapacity();
    try w.bytes.appendSlice(self.c.allocator, out.bytes.items);
    w.mem_size = out.mem_size;
    if (f.return_prop) |rp| {
        var r: TypeInfo = .{ .prop_type = rp.prop_type, .array_dim = rp.array_dim };
        r.struct_ = rp.struct_;
        r.property_class = rp.property_class;
        return r;
    }
    return .{ .prop_type = .none };
}

/// Dynamic array operations (Add/Remove/Insert/Find/...).
fn compileDynArrayOp(self: *Parser, w: *bytecode.Writer, base: TypeInfo, member: []const u8) CompileError!TypeInfo {
    _ = base;
    const base_bytes = w.bytes.items;
    var out = bytecode.Writer.init(self.c.allocator);
    defer out.deinit();
    const op: opcodes.Expr = if (std.mem.eql(u8, member, "Add"))
        .dyn_array_add
    else if (std.mem.eql(u8, member, "Remove"))
        .dyn_array_remove
    else if (std.mem.eql(u8, member, "Insert"))
        .dyn_array_insert
    else if (std.mem.eql(u8, member, "RemoveItem"))
        .dyn_array_remove_item
    else if (std.mem.eql(u8, member, "InsertItem"))
        .dyn_array_insert_item
    else
        .dyn_array_add_item;
    try out.opcode(op);
    try out.bytes.appendSlice(self.c.allocator, base_bytes);
    // Consume comma-separated arguments (count varies per operation) until `)`.
    try self.expectSym("(");
    while (!std.mem.eql(u8, self.peekSym(), ")")) {
        const p = try self.compileExprInternal(.{}, MAXINT);
        try out.bytes.appendSlice(self.c.allocator, p.bytes);
        out.mem_size += p.mem_size;
        self.c.allocator.free(p.bytes);
        if (!self.matchSym(",")) break;
    }
    try self.expectSym(")");
    try out.opcode(.end_function_parms);

    w.bytes.clearRetainingCapacity();
    try w.bytes.appendSlice(self.c.allocator, out.bytes.items);
    w.mem_size = out.mem_size;
    return .{ .prop_type = .int_, .flags = opcodes.cp.out_parm };
}

/// Compile a single atom (literal, identifier, paren, cast) into `w`.
fn compileAtom(self: *Parser, w: *bytecode.Writer) CompileError!TypeInfo {
    const t = self.peek();
    switch (t.kind) {
        .number => {
            self.pos += 1;
            return self.compileNumber(w, t.text);
        },
        .string => {
            self.pos += 1;
            try w.opcode(.string_const);
            try w.stringConst(t.text);
            return .{ .prop_type = .string };
        },
        .name_const => {
            self.pos += 1;
            try w.opcode(.name_const);
            try w.name(try self.c.nameIndex(t.text), 0);
            return .{ .prop_type = .name };
        },
        .object_const => {
            self.pos += 1;
            return self.compileObjectConst(w, t);
        },
        .symbol => {
            if (self.matchSym("(")) {
                const e = try self.compileExprInternal(.{}, MAXINT);
                try self.expectSym(")");
                const rtype = e.type;
                try w.bytes.appendSlice(self.c.allocator, e.bytes);
                w.mem_size += e.mem_size;
                self.c.allocator.free(e.bytes);
                return rtype;
            }
            if (self.matchSym("-")) {
                const e = try self.compileExprInternal(.{}, MAXINT);
                return self.emitUnary(w, "-", e);
            }
            if (self.matchSym("!")) {
                const e = try self.compileExprInternal(.{}, MAXINT);
                return self.emitUnary(w, "!", e);
            }
            if (self.matchSym("~")) {
                const e = try self.compileExprInternal(.{}, MAXINT);
                return self.emitUnary(w, "~", e);
            }
            if (self.matchSym("++")) {
                const e = try self.compileExprInternal(.{}, MAXINT);
                return self.emitUnary(w, "++", e);
            }
            if (self.matchSym("--")) {
                const e = try self.compileExprInternal(.{}, MAXINT);
                return self.emitUnary(w, "--", e);
            }
            return self.errFmt(t, "unexpected symbol '{s}' in expression", .{t.text});
        },
        .identifier => {
            // Casts: `int(...)`, `float(...)`, etc.
            const cast_type = self.peekCastType(t.text);
            if (cast_type) |ct| {
                if (self.pos + 1 < self.toks.len and
                    self.toks[self.pos + 1].kind == .symbol and
                    std.mem.eql(u8, self.toks[self.pos + 1].text, "("))
                {
                    self.pos += 1; // consume type name
                    try self.expectSym("("); // the cast's argument delimiter
                    const inner = try self.compileExprInternal(.{}, MAXINT);
                    try self.expectSym(")");
                    return self.emitCast(w, ct, inner);
                }
            }
            return self.compileIdentifierAtom(w, t);
        },
        .eof => return self.errFmt(t, "unexpected end of input in expression", .{}),
    }
}

fn peekCastType(_: *Parser, name: []const u8) ?opcodes.PropType {
    const names = [_]struct { n: []const u8, t: opcodes.PropType }{
        .{ .n = "byte", .t = .byte },
        .{ .n = "int", .t = .int_ },
        .{ .n = "bool", .t = .bool_ },
        .{ .n = "float", .t = .float_ },
        .{ .n = "string", .t = .string },
        .{ .n = "name", .t = .name },
        .{ .n = "vector", .t = .struct_ },
        .{ .n = "rotator", .t = .struct_ },
    };
    for (names) |e| {
        if (std.mem.eql(u8, name, e.n)) return e.t;
    }
    return null;
}

fn emitCast(self: *Parser, w: *bytecode.Writer, dest_type: opcodes.PropType, inner: Compiled) CompileError!TypeInfo {
    const dest: TypeInfo = .{ .prop_type = dest_type };
    if (inner.type.prop_type == dest_type) {
        // No-op cast (or vector/rotator).
        try w.bytes.appendSlice(self.c.allocator, inner.bytes);
        self.c.allocator.free(inner.bytes);
        return inner.type;
    }
    const cast_token = types.conversion(dest, inner.type) orelse {
        self.c.allocator.free(inner.bytes);
        return self.errFmt(self.peek(), "can't convert {s} to {s}", .{ propTypeName(inner.type), propTypeName(dest) });
    };
    try w.opcode(.primitive_cast);
    try w.byte(@intFromEnum(cast_token));
    try w.bytes.appendSlice(self.c.allocator, inner.bytes);
    self.c.allocator.free(inner.bytes);
    return dest;
}

fn compileNumber(self: *Parser, w: *bytecode.Writer, text: []const u8) CompileError!TypeInfo {
    // Heuristic: contains a '.', 'e', or trailing 'f' -> float.
    var is_float = false;
    for (text) |c| {
        if (c == '.' or c == 'e' or c == 'E') is_float = true;
    }
    if (text.len > 0 and (text[text.len - 1] == 'f' or text[text.len - 1] == 'F')) is_float = true;

    if (is_float) {
        const v: f32 = @floatCast(try self.parseFloat(text));
        try w.opcode(.float_const);
        try w.float(v);
        return .{ .prop_type = .float_ };
    }
    const v: i64 = try self.parseInt(text);
    const iv: i32 = @intCast(v);
    if (iv == 0) {
        try w.opcode(.int_zero);
    } else if (iv == 1) {
        try w.opcode(.int_one);
    } else if (iv >= 0 and iv <= 255) {
        try w.opcode(.int_const_byte);
        try w.byte(@intCast(iv));
    } else {
        try w.opcode(.int_const);
        try w.int32(iv);
    }
    return .{ .prop_type = .int_ };
}

fn compileObjectConst(self: *Parser, w: *bytecode.Writer, t: Token) CompileError!TypeInfo {
    // `Class'Name'` -> a class constant; other object consts are unsupported
    // as literals (would need package resolution).
    if (std.mem.eql(u8, t.obj_class, "Class")) {
        try w.opcode(.object_const);
        // Reference the class's export index (or 0 if unknown).
        const cls = self.c.findClass(t.text);
        const idx: i32 = if (cls) |c| c.export_index + 1 else 0;
        try w.object(idx);
        return .{
            .prop_type = .object_reference,
            .property_class = "Class",
            .meta_class = t.text,
        };
    }
    try w.opcode(.object_const);
    try w.object(0);
    return .{ .prop_type = .object_reference, .property_class = t.obj_class };
}

/// Compile an identifier that is not a cast keyword.
fn compileIdentifierAtom(self: *Parser, w: *bytecode.Writer, t: Token) CompileError!TypeInfo {
    const name = t.text;

    // Keyword literals / contexts.
    if (kwEql(name, "true")) {
        self.pos += 1;
        try w.opcode(.true_);
        return .{ .prop_type = .bool_ };
    }
    if (kwEql(name, "false")) {
        self.pos += 1;
        try w.opcode(.false_);
        return .{ .prop_type = .bool_ };
    }
    if (kwEql(name, "none")) {
        self.pos += 1;
        try w.opcode(.no_object);
        return .{ .prop_type = .object_reference };
    }
    if (kwEql(name, "self")) {
        self.pos += 1;
        try w.opcode(.self);
        const cls = self.c.cur_class.?;
        return .{ .prop_type = .object_reference, .property_class = cls.name };
    }
    if (kwEql(name, "vect")) {
        self.pos += 1;
        try self.expectSym("(");
        var v: [3]f32 = .{ 0, 0, 0 };
        for (0..3) |i| {
            if (i > 0) try self.expectSym(",");
            const num = try self.ident();
            const val: f32 = @floatCast(try self.parseFloat(num));
            v[i] = val;
        }
        try self.expectSym(")");
        try w.opcode(.vector_const);
        try w.float(v[0]);
        try w.float(v[1]);
        try w.float(v[2]);
        return try self.builtinStructType("Vector");
    }
    if (kwEql(name, "rot")) {
        self.pos += 1;
        try self.expectSym("(");
        var r: [3]i32 = .{ 0, 0, 0 };
        for (0..3) |i| {
            if (i > 0) try self.expectSym(",");
            const num = try self.ident();
            r[i] = @intCast(try self.parseInt(num));
        }
        try self.expectSym(")");
        try w.opcode(.rotation_const);
        try w.int32(r[0]);
        try w.int32(r[1]);
        try w.int32(r[2]);
        return try self.builtinStructType("Rotator");
    }
    if (kwEql(name, "default")) {
        // `default.Member`
        self.pos += 1;
        try self.expectSym(".");
        const member = try self.ident();
        const cls = self.c.cur_class.?;
        const f = cls.findProperty(member) orelse
            return self.errFmt(t, "unknown property '{s}'", .{member});
        try w.opcode(.default_variable);
        try w.object(try self.propRef(f));
        var r: TypeInfo = .{ .prop_type = f.prop_type, .array_dim = f.array_dim };
        r.enum_ = f.enum_;
        r.property_class = f.property_class;
        r.struct_ = f.struct_;
        return r;
    }
    if (kwEql(name, "new")) {
        self.pos += 1;
        return self.compileNew(w);
    }
    if (kwEql(name, "super")) {
        self.pos += 1;
        return self.compileSuper(w);
    }
    if (kwEql(name, "global")) {
        self.pos += 1;
        try self.expectSym(".");
        return self.compileGlobal(w);
    }
    if (kwEql(name, "static")) {
        // `static.Member` inside a static function.
        self.pos += 1;
        try self.expectSym(".");
        const member = try self.ident();
        const cls = self.c.cur_class.?;
        if (cls.findFunction(member)) |f| {
            return self.emitDirectCall(w, f);
        }
        return self.errFmt(t, "unknown static member '{s}'", .{member});
    }

    // Enum value: `EnumName.Value` or a bare enum tag.
    if (self.pos + 1 < self.toks.len and
        self.toks[self.pos + 1].kind == .symbol and
        std.mem.eql(u8, self.toks[self.pos + 1].text, "."))
    {
        if (self.c.findEnum(name)) |e| {
            self.pos += 1;
            try self.expectSym(".");
            const val_name = try self.ident();
            const idx = self.enumIndex(e, val_name) orelse
                return self.errFmt(t, "unknown enum value '{s}.{s}'", .{ name, val_name });
            try w.opcode(.byte_const);
            try w.byte(@intCast(idx));
            return .{ .prop_type = .byte, .enum_ = e };
        }
    }

    // Variable or function reference.
    return self.compileVariableRef(w, t);
}

fn compileVariableRef(self: *Parser, w: *bytecode.Writer, t: Token) CompileError!TypeInfo {
    const name = t.text;
    self.pos += 1;

    // Function call: `Name(`.
    if (self.peekSym().len > 0 and std.mem.eql(u8, self.peekSym(), "(")) {
        if (self.c.cur_class) |cls| {
            if (cls.findFunction(name)) |f| {
                self.pos += 1;
                return self.emitDirectCall(w, f);
            }
        }
        // Unknown (native/global) function: emit a virtual call dispatched by
        // name at runtime. Arguments are compiled without type-checking.
        return self.emitUnknownCall(w, name);
    }

    // Local or param.
    if (self.c.cur_function) |f| {
        for (f.params.items) |p| {
            if (std.mem.eql(u8, p.name, name)) {
                return self.emitLocalRef(w, p);
            }
        }
        for (f.locals.items) |p| {
            if (std.mem.eql(u8, p.name, name)) {
                return self.emitLocalRef(w, p);
            }
        }
    }

    // Class member.
    if (self.c.cur_class) |cls| {
        if (cls.findProperty(name)) |p| {
            try w.opcode(.instance_variable);
            try w.object(try self.propRef(p));
            var r: TypeInfo = .{ .prop_type = p.prop_type, .array_dim = p.array_dim };
            r.enum_ = p.enum_;
            r.property_class = p.property_class;
            r.struct_ = p.struct_;
            r.flags = opcodes.cp.out_parm;
            return r;
        }
    }

    // Built-in UObject properties (not part of any compiled class).
    if (kwEql(name, "Outer")) {
        try w.opcode(.instance_variable);
        try w.object(0); // unresolved; the linker resolves UObject::Outer
        return .{ .prop_type = .object_reference, .property_class = null, .flags = opcodes.cp.out_parm };
    }
    if (kwEql(name, "Class")) {
        try w.opcode(.instance_variable);
        try w.object(0);
        return .{ .prop_type = .object_reference, .property_class = "Class", .flags = opcodes.cp.out_parm };
    }

    return self.errFmt(t, "unknown variable '{s}'", .{name});
}

/// Emit a call to a function not declared in any compiled class (a native
/// global, engine function, or one from an unloaded package). Emitted as
/// EX_VirtualFunction + name + uncompiled arguments + EndFunctionParms. The
/// result is opaque; the VM resolves the function by name at runtime.
fn emitUnknownCall(self: *Parser, w: *bytecode.Writer, name: []const u8) CompileError!TypeInfo {
    self.pos += 1; // (
    try w.opcode(.virtual_function);
    try w.name(try self.c.nameIndex(name), 0);
    var depth: usize = 1;
    while (self.peekKind() != .eof and depth > 0) {
        const s = self.peekSym();
        if (std.mem.eql(u8, s, "(")) {
            depth += 1;
            self.pos += 1;
        } else if (std.mem.eql(u8, s, ")")) {
            depth -= 1;
            if (depth == 0) {
                self.pos += 1;
                break;
            }
            self.pos += 1;
        } else if (std.mem.eql(u8, s, ",")) {
            // Skip argument tokens without type-checking.
            self.pos += 1;
        } else {
            self.pos += 1;
        }
    }
    try w.opcode(.end_function_parms);
    return .{ .prop_type = .none, .unknown = true };
}

fn emitLocalRef(self: *Parser, w: *bytecode.Writer, p: *model.Property) CompileError!TypeInfo {
    if (p.is_out) {
        try w.opcode(.local_out_variable);
    } else {
        try w.opcode(.local_variable);
    }
    try w.object(try self.propRef(p));
    var r: TypeInfo = .{ .prop_type = p.prop_type, .array_dim = p.array_dim };
    r.enum_ = p.enum_;
    r.property_class = p.property_class;
    r.struct_ = p.struct_;
    r.flags = opcodes.cp.out_parm;
    return r;
}

/// Emit a direct (non-context) function call.
fn emitDirectCall(self: *Parser, w: *bytecode.Writer, f: *model.Function) CompileError!TypeInfo {
    // Native final function: emit its native index byte directly.
    if (f.i_native > 0 and f.i_native < 256 and (f.flags & opcodes.func.final_ != 0)) {
        try w.byte(@intCast(f.i_native));
    } else if (f.flags & opcodes.func.final_ != 0) {
        try w.opcode(.final_function);
        try w.object(try self.funcRef(f));
    } else {
        try w.opcode(.virtual_function);
        try w.name(try self.c.nameIndex(f.name), 0);
    }

    // Arguments.
    var count: usize = 0;
    for (f.params.items) |p| {
        if (count > 0) {
            if (!self.matchSym(",")) {
                if (p.is_optional) {
                    try w.opcode(.empty_parm_value);
                    continue;
                }
                return self.errFmt(self.peek(), "expected ',' before parameter {d}", .{count + 1});
            }
        }
        if (std.mem.eql(u8, self.peekSym(), ")")) {
            if (p.is_optional) {
                try w.opcode(.empty_parm_value);
                continue;
            }
        }
        const required: TypeInfo = .{
            .prop_type = p.prop_type,
            .array_dim = p.array_dim,
            .enum_ = p.enum_,
            .property_class = p.property_class,
            .struct_ = p.struct_,
        };
        const arg = try self.compileExprInternal(required, MAXINT);
        try w.bytes.appendSlice(self.c.allocator, arg.bytes);
        w.mem_size += arg.mem_size;
        self.c.allocator.free(arg.bytes);
        count += 1;
    }
    try self.expectSym(")");
    try w.opcode(.end_function_parms);

    if (f.return_prop) |rp| {
        // Record the return prop so unused results can be eaten.
        self.affector_return_prop = try self.propRef(rp);
        var r: TypeInfo = .{ .prop_type = rp.prop_type, .array_dim = rp.array_dim };
        r.enum_ = rp.enum_;
        r.property_class = rp.property_class;
        r.struct_ = rp.struct_;
        return r;
    }
    self.affector_return_prop = null;
    return .{ .prop_type = .none };
}

/// `new [Class](...)` and `new (Outer[,Name[,Flags]]) Class(...)`.
fn compileNew(self: *Parser, w: *bytecode.Writer) CompileError!TypeInfo {
    try w.opcode(.new_);
    // Parent expression.
    if (self.matchSym("(")) {
        const parent = try self.compileExprInternal(.{}, MAXINT);
        try w.bytes.appendSlice(self.c.allocator, parent.bytes);
        w.mem_size += parent.mem_size;
        self.c.allocator.free(parent.bytes);
        if (self.matchSym(",")) {
            const name_expr = try self.compileExprInternal(.{}, MAXINT);
            try w.bytes.appendSlice(self.c.allocator, name_expr.bytes);
            w.mem_size += name_expr.mem_size;
            self.c.allocator.free(name_expr.bytes);
            if (self.matchSym(",")) {
                const flags = try self.compileExprInternal(.{}, MAXINT);
                try w.bytes.appendSlice(self.c.allocator, flags.bytes);
                w.mem_size += flags.mem_size;
                self.c.allocator.free(flags.bytes);
            } else {
                try w.opcode(.nothing);
            }
        } else {
            try w.opcode(.nothing);
        }
        try self.expectSym(")");
    } else {
        try w.opcode(.nothing);
    }
    // Name + flags + class.
    try w.opcode(.nothing); // name
    try w.opcode(.nothing); // flags
    const class_ti = try self.compileExprInternal(.{}, MAXINT);
    try w.bytes.appendSlice(self.c.allocator, class_ti.bytes);
    w.mem_size += class_ti.mem_size;
    self.c.allocator.free(class_ti.bytes);
    // Constructor params.
    if (self.matchSym("(")) {
        _ = try self.compileExpr(MAXINT);
        try self.expectSym(")");
    }
    return .{ .prop_type = .object_reference };
}

/// `super[.Parent].Func(...)`.
fn compileSuper(self: *Parser, w: *bytecode.Writer) CompileError!TypeInfo {
    var super_class: ?*model.ClassObj = null;
    if (self.matchSym("(")) {
        const name = try self.ident();
        try self.expectSym(")");
        super_class = self.c.findClass(name);
    }
    try self.expectSym(".");
    const member = try self.ident();
    const cls = self.c.cur_class.?;
    const base = super_class orelse self.c.findClass(cls.super_name) orelse
        return self.errFmt(self.peek(), "no superclass for '{s}'", .{cls.name});
    if (base.findFunction(member)) |f| {
        // Super calls bind to the parent version: EX_FinalFunction + ref.
        try w.opcode(.final_function);
        try w.object(try self.funcRef(f));
        // Args.
        var count: usize = 0;
        for (f.params.items) |p| {
            if (count > 0) _ = self.matchSym(",");
            if (std.mem.eql(u8, self.peekSym(), ")")) {
                try w.opcode(.empty_parm_value);
                continue;
            }
            const required: TypeInfo = .{ .prop_type = p.prop_type, .array_dim = p.array_dim, .struct_ = p.struct_, .property_class = p.property_class };
            const arg = try self.compileExprInternal(required, MAXINT);
            try w.bytes.appendSlice(self.c.allocator, arg.bytes);
            w.mem_size += arg.mem_size;
            self.c.allocator.free(arg.bytes);
            count += 1;
        }
        try self.expectSym(")");
        try w.opcode(.end_function_parms);
        if (f.return_prop) |rp| {
            var r: TypeInfo = .{ .prop_type = rp.prop_type, .array_dim = rp.array_dim };
            r.struct_ = rp.struct_;
            r.property_class = rp.property_class;
            return r;
        }
    }
    return .{ .prop_type = .none };
}

/// `global.Func(...)` — the non-state version of a function.
fn compileGlobal(self: *Parser, w: *bytecode.Writer) CompileError!TypeInfo {
    const name = try self.ident();
    const cls = self.c.cur_class.?;
    if (cls.findFunction(name)) |f| {
        try w.opcode(.global_function);
        try w.name(try self.c.nameIndex(f.name), 0);
        var count: usize = 0;
        for (f.params.items) |p| {
            if (count > 0) _ = self.matchSym(",");
            const required: TypeInfo = .{ .prop_type = p.prop_type, .array_dim = p.array_dim, .struct_ = p.struct_ };
            const arg = try self.compileExprInternal(required, MAXINT);
            try w.bytes.appendSlice(self.c.allocator, arg.bytes);
            w.mem_size += arg.mem_size;
            self.c.allocator.free(arg.bytes);
            count += 1;
        }
        try self.expectSym(")");
        try w.opcode(.end_function_parms);
    }
    return .{ .prop_type = .none };
}

/// Emit a unary operator (pre).
fn emitUnary(self: *Parser, w: *bytecode.Writer, op_name: []const u8, e: Compiled) CompileError!TypeInfo {
    var oper: ?*const types.Operator = null;
    for (&types.operators) |*op| {
        if (op.pre and kwEql(op.name, op_name) and typeMatches(op.left, op.left_struct, e.type)) {
            oper = op;
            break;
        }
    }
    const op = oper orelse {
        self.c.allocator.free(e.bytes);
        return self.errFmt(self.peek(), "no preoperator '{s}' for {s}", .{ op_name, propTypeName(e.type) });
    };
    // Operator call: native byte + operand + EndFunctionParms.
    try w.byte(@intCast(@as(u8, @intCast(op.native_index))));
    try w.bytes.appendSlice(self.c.allocator, e.bytes);
    self.c.allocator.free(e.bytes);
    try w.opcode(.end_function_parms);
    return operatorReturnType(op, e.type, .{});
}

fn enumIndex(_: *Parser, e: *model.Enum, value: []const u8) ?i32 {
    for (e.values, 0..) |v, i| {
        if (std.mem.eql(u8, v, value)) return @intCast(i);
    }
    return null;
}

fn builtinStructType(self: *Parser, name: []const u8) CompileError!TypeInfo {
    const s = self.c.allocator.create(model.ScriptStruct) catch return error.OutOfMemory;
    s.* = .{
        .export_index = -1,
        .name = name,
        .flags = 0,
        .super_name = "",
        .fields = std.ArrayList(*model.Property).empty,
        .offset = 12,
    };
    return .{ .prop_type = .struct_, .struct_ = s };
}

/// Compile a list of call arguments into `w` for an already-opened call.
fn compileCallArgs(self: *Parser, w: *bytecode.Writer, f: *model.Function) CompileError!TypeInfo {
    var count: usize = 0;
    for (f.params.items) |p| {
        if (count > 0) {
            if (!self.matchSym(",")) {
                if (p.is_optional) {
                    try w.opcode(.empty_parm_value);
                    continue;
                }
                return self.errFmt(self.peek(), "expected ',' before parameter {d}", .{count + 1});
            }
        }
        const required: TypeInfo = .{ .prop_type = p.prop_type, .array_dim = p.array_dim, .struct_ = p.struct_, .property_class = p.property_class };
        const arg = try self.compileExprInternal(required, MAXINT);
        try w.bytes.appendSlice(self.c.allocator, arg.bytes);
        w.mem_size += arg.mem_size;
        self.c.allocator.free(arg.bytes);
        count += 1;
    }
    try self.expectSym(")");
    try w.opcode(.end_function_parms);
    if (f.return_prop) |rp| {
        var r: TypeInfo = .{ .prop_type = rp.prop_type, .array_dim = rp.array_dim };
        r.enum_ = rp.enum_;
        r.struct_ = rp.struct_;
        r.property_class = rp.property_class;
        return r;
    }
    return .{ .prop_type = .none };
}

/// Collect the value text of a default-property assignment up to the next
/// top-level `;` (respecting nested `()`, `[]`, `{}`).
fn collectValueText(self: *Parser) CompileError![]const u8 {
    var depth: usize = 0;
    const start = self.pos;
    while (self.peekKind() != .eof) {
        const s = self.peekSym();
        if (s.len > 0) {
            if (std.mem.eql(u8, s, "(") or std.mem.eql(u8, s, "[") or std.mem.eql(u8, s, "{")) {
                depth += 1;
            } else if (std.mem.eql(u8, s, ")") or std.mem.eql(u8, s, "]") or std.mem.eql(u8, s, "}")) {
                if (depth == 0) break;
                depth -= 1;
            } else if (depth == 0 and std.mem.eql(u8, s, ";")) {
                break;
            }
        } else if (depth == 0 and (self.peekKind() == .eof)) {
            break;
        }
        self.pos += 1;
    }
    // Reconstruct approximate source text from tokens.
    var buf = std.ArrayList(u8).empty;
    for (self.toks[start..self.pos]) |t| {
        switch (t.kind) {
            .symbol, .identifier, .number => {
                buf.appendSlice(self.c.allocator, t.text) catch return error.OutOfMemory;
            },
            .string, .name_const => {
                buf.append(self.c.allocator, '\'') catch return error.OutOfMemory;
                buf.appendSlice(self.c.allocator, t.text) catch return error.OutOfMemory;
                buf.append(self.c.allocator, '\'') catch return error.OutOfMemory;
            },
            else => {},
        }
        buf.append(self.c.allocator, ' ') catch return error.OutOfMemory;
    }
    return buf.toOwnedSlice(self.c.allocator) catch return error.OutOfMemory;
}
