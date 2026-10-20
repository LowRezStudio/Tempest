// End-to-end tests for the UnrealScript compiler: lex, compile, serialize, and
// re-parse a generated package.

const std = @import("std");

const lexer_mod = @import("lexer.zig");
const compile = @import("compile.zig");
const frontend = @import("frontend.zig");
const package = @import("package.zig");
const unreal = @import("unreal");

fn compileString(src: []const u8, allocator: std.mem.Allocator) !*compile.Compiler {
    const c = try allocator.create(compile.Compiler);
    c.* = compile.Compiler.init(allocator, "TestPkg");
    errdefer {
        c.deinit();
        allocator.destroy(c);
    }
    const cls = try frontend.compileSource(c, src, "Test.uc");
    _ = cls;
    return c;
}

test "lexer tokenizes a class declaration" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();
    var lx = lexer_mod.Lexer.init("class Foo extends Object { var int X; }");
    const toks = try lx.tokenize(allocator);
    try std.testing.expect(toks.len >= 10);
    try std.testing.expectEqual(lexer_mod.TokenKind.identifier, toks[0].kind);
    try std.testing.expectEqualStrings("class", toks[0].text);
    // The last token is EOF.
    try std.testing.expectEqual(lexer_mod.TokenKind.eof, toks[toks.len - 1].kind);
}

test "lexer handles negative numbers and name constants" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();
    var lx = lexer_mod.Lexer.init("x = -1; y = 'Name';");
    const toks = try lx.tokenize(allocator);
    var saw_minus: bool = false;
    var saw_name: bool = false;
    for (toks) |t| {
        if (t.kind == .number and std.mem.eql(u8, t.text, "-1")) saw_minus = true;
        if (t.kind == .name_const and std.mem.eql(u8, t.text, "Name")) saw_name = true;
    }
    try std.testing.expect(saw_minus);
    try std.testing.expect(saw_name);
}

test "compiler produces a package that parses back" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();
    const src =
        \\class TestThing extends Object
        \\    config(Game);
        \\
        \\var int Health;
        \\var float Speed;
        \\var string Name;
        \\var bool bAlive;
        \\var array<int> Scores;
        \\var EColor Shade;
        \\var Point Origin;
        \\
        \\enum EColor
        \\{
        \\    COLOR_Red,
        \\    COLOR_Green,
        \\    COLOR_Blue,
        \\};
        \\
        \\struct Point
        \\{
        \\    var float X;
        \\    var float Y;
        \\};
        \\
        \\function int GetHealth()
        \\{
        \\    return Health;
        \\}
        \\
        \\simulated function SetHealth(int NewHealth)
        \\{
        \\    Health = NewHealth;
        \\}
        \\
        \\function float ComputeBonus()
        \\{
        \\    local int base;
        \\    base = 100;
        \\    return Speed * base;
        \\}
        \\
        \\function bool IsAlive()
        \\{
        \\    return bAlive && (Health > 0);
        \\}
        \\
        \\defaultproperties
        \\{
        \\    Health=100
        \\    Speed=10.5
        \\}
        \\
    ;
    const c = try compileString(src, allocator);
    defer {
        c.deinit();
        allocator.destroy(c);
    }

    const bytes = try package.buildPackage(c, allocator);
    try std.testing.expect(bytes.len > 100);

    // Re-parse the package summary with the existing UPK parser.
    var r: std.Io.Reader = .fixed(bytes);
    const summary = try unreal.PackageFileSummary.take(&r, allocator);
    defer summary.deinit(allocator);

    try std.testing.expectEqual(@as(i32, 893), summary.file_version);
    try std.testing.expect(summary.name_count > 10);
    try std.testing.expect(summary.export_count > 10);
    try std.testing.expect(summary.import_count > 0);
    // The export data must fit within the produced file.
    try std.testing.expect(summary.total_header_size > 0 and summary.total_header_size < bytes.len);
}

test "compiled function bytecode is well-formed" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();
    const src =
        \\class F extends Object;
        \\function int Add(int A, int B)
        \\{
        \\    return A + B;
        \\}
        \\function void Set(int V)
        \\{
        \\    V = 5;
        \\}
        \\
    ;
    const c = try compileString(src, allocator);
    defer {
        c.deinit();
        allocator.destroy(c);
    }

    const cls = c.findClass("F").?;
    try std.testing.expectEqual(@as(usize, 2), cls.functions.items.len);

    // The Add function: EX_Return EX_LocalVariable(0) ... EndOfScript.
    const add_fn = cls.functions.items[0];
    try std.testing.expect(add_fn.script.items.len > 4);
    // The first bytecode opcode should be a return (0x04) since the body is a
    // single return statement.
    try std.testing.expectEqual(@as(u8, 0x04), add_fn.script.items[0]);
    // The script must end with EX_EndOfScript.
    try std.testing.expectEqual(@as(u8, 0x53), add_fn.script.items[add_fn.script.items.len - 1]);
}
