// The UnrealScript compiler: parses .uc source into the object model and
// emits bytecode. This mirrors FScriptCompiler in UnrealEd/Src/UnScrCom.cpp,
// scaled to the core language.

const std = @import("std");

const lexer_mod = @import("lexer.zig");
const opcodes = @import("opcodes.zig");
const model = @import("model.zig");

const Token = lexer_mod.Token;
const TokenKind = lexer_mod.TokenKind;
const Lexer = lexer_mod.Lexer;

pub const CompileError = error{
    Syntax,
    UnknownType,
    UnknownField,
    UnknownFunction,
    UnknownClass,
    TypeMismatch,
    InvalidCast,
    DuplicateName,
    NotAVariable,
    BadExpression,
    BadStatement,
    CodeSpaceOverflow,
    OutOfMemory,
} || std.mem.Allocator.Error;

/// One import-map entry.
pub const ImportEntry = struct {
    class_package: []const u8,
    class_name: []const u8,
    outer_index: i32,
    object_name: []const u8,
};

/// A resolved reference to an object in the package (import/export/None).
pub const Ref = struct {
    /// Package index: >0 = export, <0 = import, 0 = None.
    index: i32,

    pub const none: Ref = .{ .index = 0 };
};

/// The compiler. One instance compiles a set of .uc files into a package.
pub const Compiler = struct {
    allocator: std.mem.Allocator,

    // Name table (shared across the whole package).
    names: std.ArrayList([]const u8),
    name_map: std.StringHashMap(u32),

    // Import map.
    imports: std.ArrayList(ImportEntry),
    import_map: std.StringHashMap(i32),

    // Compiled classes (the exports).
    classes: std.ArrayList(*model.ClassObj),

    /// Next export-map index to hand out.
    next_export: i32 = 0,

    // The package we are building.
    package_name: []const u8,

    // Current compile context.
    cur_class: ?*model.ClassObj = null,
    cur_function: ?*model.Function = null,
    cur_state: ?*model.State = null,

    // Property layout: current running offset within the class.
    // (Properties are laid out in declaration order.)
    // Errors accumulate here; the compiler reports them with line info.
    err: ?[]const u8 = null,
    err_line: u32 = 0,

    pub fn init(allocator: std.mem.Allocator, package_name: []const u8) Compiler {
        return .{
            .allocator = allocator,
            .names = std.ArrayList([]const u8).empty,
            .name_map = std.StringHashMap(u32).init(allocator),
            .imports = std.ArrayList(ImportEntry).empty,
            .import_map = std.StringHashMap(i32).init(allocator),
            .classes = std.ArrayList(*model.ClassObj).empty,
            .package_name = package_name,
        };
    }

    pub fn deinit(self: *Compiler) void {
        self.names.deinit(self.allocator);
        self.name_map.deinit();
        self.imports.deinit(self.allocator);
        self.import_map.deinit();
        for (self.classes.items) |c| c.deinit(self.allocator);
        self.classes.deinit(self.allocator);
    }

    /// Register a name, returning its package name-map index.
    pub fn nameIndex(self: *Compiler, name: []const u8) CompileError!u32 {
        if (self.name_map.get(name)) |idx| return idx;
        const copy = self.allocator.dupe(u8, name) catch return error.OutOfMemory;
        const idx: u32 = @intCast(self.names.items.len);
        self.names.append(self.allocator, copy) catch return error.OutOfMemory;
        self.name_map.put(copy, idx) catch return error.OutOfMemory;
        return idx;
    }

    /// Get or create an import. Returns the package index (negative).
    fn importIndex(
        self: *Compiler,
        class_package: []const u8,
        class_name: []const u8,
        outer_index: i32,
        object_name: []const u8,
    ) CompileError!i32 {
        const key = std.fmt.allocPrint(self.allocator, "{s}.{s}.{d}.{s}", .{
            class_package, class_name, outer_index, object_name,
        }) catch return error.OutOfMemory;
        if (self.import_map.get(key)) |idx| return idx;

        // Register the import's names in the name table so they are present
        // before the name map is serialized.
        _ = try self.nameIndex(class_package);
        _ = try self.nameIndex(class_name);
        _ = try self.nameIndex(object_name);

        self.imports.append(self.allocator, .{
            .class_package = class_package,
            .class_name = class_name,
            .outer_index = outer_index,
            .object_name = object_name,
        }) catch return error.OutOfMemory;
        const idx: i32 = -@as(i32, @intCast(self.imports.items.len));
        self.import_map.put(key, idx) catch return error.OutOfMemory;
        return idx;
    }

    /// Import a core class by name (Core.<Name>).
    pub fn importCore(self: *Compiler, class_name: []const u8) CompileError!i32 {
        return self.importIndex("Core", "Class", 0, class_name);
    }

    /// Import a property class by name (Core.<Name>Property).
    pub fn importPropertyClass(self: *Compiler, class_name: []const u8) CompileError!i32 {
        return self.importIndex("Core", class_name, 0, class_name);
    }

    /// Find a compiled class by name.
    pub fn findClass(self: *Compiler, name: []const u8) ?*model.ClassObj {
        for (self.classes.items) |c| {
            if (std.mem.eql(u8, c.name, name)) return c;
        }
        return null;
    }

    /// Find an enum declared in any compiled class.
    pub fn findEnum(self: *Compiler, name: []const u8) ?*model.Enum {
        for (self.classes.items) |c| {
            if (c.findEnum(name)) |e| return e;
        }
        return null;
    }

    /// Find a struct declared in any compiled class.
    pub fn findStruct(self: *Compiler, name: []const u8) ?*model.ScriptStruct {
        for (self.classes.items) |c| {
            if (c.findStruct(name)) |s| return s;
        }
        return null;
    }

    /// Resolve the base class of `name` (following `extends` within the
    /// current package), returning null for external superclasses.
    pub fn superOf(self: *Compiler, name: []const u8) ?*model.ClassObj {
        const c = self.findClass(name) orelse return null;
        return self.findClass(c.super_name);
    }

    /// Assign the next export index.
    pub fn nextExport(self: *Compiler) i32 {
        const idx = self.next_export;
        self.next_export += 1;
        return idx;
    }

    /// Report a compile error.
    fn fail(self: *Compiler, line: u32, comptime fmt: []const u8, args: anytype) CompileError {
        self.err = std.fmt.allocPrint(self.allocator, fmt, args) catch return error.OutOfMemory;
        self.err_line = line;
        return error.Syntax;
    }
};
