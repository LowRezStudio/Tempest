pub const minilzo = @import("minilzo");

test {
    _ = @import("upk-parser/Parser.zig");
    _ = @import("unreal");
    _ = @import("upk-parser/compression.zig");
    _ = @import("upk-parser/property.zig");
    _ = @import("compiler/lexer.zig");
    _ = @import("compiler/compile.zig");
    _ = @import("compiler/frontend.zig");
    _ = @import("compiler/package.zig");
    _ = @import("compiler/tests.zig");
}
