// Compile-time type information and the built-in operator table.

const std = @import("std");
const opcodes = @import("opcodes.zig");
const model = @import("model.zig");

/// The compile-time representation of a value's type (mirrors FPropertyBase).
pub const TypeInfo = struct {
    prop_type: opcodes.PropType = .none,
    /// 1 = scalar, 0 = dynamic array, >1 = static array.
    array_dim: i32 = 1,
    flags: u64 = 0,
    /// Byte property backed by an enum.
    enum_: ?*model.Enum = null,
    /// Object/interface target class name.
    property_class: ?[]const u8 = null,
    /// class<limiter>.
    meta_class: ?[]const u8 = null,
    /// Struct type.
    struct_: ?*model.ScriptStruct = null,
    /// Dynamic array element type.
    inner: ?*TypeInfo = null,
    /// Delegate target function name.
    delegate_function: ?[]const u8 = null,
    /// The type is genuinely unknown (a native/external call whose signature
    /// is unavailable); it matches any expected type without a cast.
    unknown: bool = false,

    pub fn isDynamicArray(self: TypeInfo) bool {
        return self.array_dim == 0;
    }

    pub fn isObject(self: TypeInfo) bool {
        return self.prop_type == .object_reference or self.prop_type == .interface;
    }

    pub fn isVector(self: TypeInfo) bool {
        return self.prop_type == .struct_ and self.struct_ != null and
            std.mem.eql(u8, self.struct_.?.name, "Vector");
    }

    pub fn isRotator(self: TypeInfo) bool {
        return self.prop_type == .struct_ and self.struct_ != null and
            std.mem.eql(u8, self.struct_.?.name, "Rotator");
    }

    /// Base element size in bytes (before array multiplication).
    pub fn elementSize(self: TypeInfo) i32 {
        switch (self.prop_type) {
            .byte, .bool_ => return 1,
            .int_, .float_, .string => return 4,
            .name => return 8,
            .object_reference, .interface, .delegate => return 8,
            .struct_ => {
                if (self.struct_) |s| return s.offset;
                return 12;
            },
            else => return 0,
        }
    }

    pub fn size(self: TypeInfo) i32 {
        return self.elementSize() * @max(self.array_dim, 1);
    }
};

/// One entry in the built-in operator table (the native operators declared in
/// Core's Object.uc).
pub const Operator = struct {
    /// The operator symbol (e.g. "+", "==", "&&").
    name: []const u8,
    /// Native function index (iNative).
    native_index: i32,
    /// Operator precedence (lower binds tighter).
    precedence: i32,
    /// Return type.
    ret: opcodes.PropType,
    /// Left parameter type (or the sole parameter for pre/post operators).
    left: opcodes.PropType,
    /// Right parameter type (0 = unary).
    right: opcodes.PropType,
    /// True if a preoperator.
    pre: bool = false,
    /// True if a postoperator.
    post: bool = false,
    /// True if the left operand is an out-param (affector).
    out: bool = false,
    /// True if the right operand is `skip` (short-circuit).
    skip: bool = false,
    /// True if params are coerced.
    coerce: bool = false,
    /// Vector/rotator/string operands are represented as CPT_Struct; we store
    /// the struct name to disambiguate.
    left_struct: ?[]const u8 = null,
    right_struct: ?[]const u8 = null,
};

/// The standard operators, from Core/Classes/Object.uc.
pub const operators = [_]Operator{
    // Bool
    .{ .name = "!", .native_index = 129, .precedence = 0, .ret = .bool_, .left = .bool_, .right = .none, .pre = true },
    .{ .name = "==", .native_index = 242, .precedence = 24, .ret = .bool_, .left = .bool_, .right = .bool_ },
    .{ .name = "!=", .native_index = 243, .precedence = 26, .ret = .bool_, .left = .bool_, .right = .bool_ },
    .{ .name = "&&", .native_index = 130, .precedence = 30, .ret = .bool_, .left = .bool_, .right = .bool_, .skip = true },
    .{ .name = "^^", .native_index = 131, .precedence = 30, .ret = .bool_, .left = .bool_, .right = .bool_ },
    .{ .name = "||", .native_index = 132, .precedence = 32, .ret = .bool_, .left = .bool_, .right = .bool_, .skip = true },

    // Int
    .{ .name = "~", .native_index = 141, .precedence = 0, .ret = .int_, .left = .int_, .right = .none, .pre = true },
    .{ .name = "-", .native_index = 143, .precedence = 0, .ret = .int_, .left = .int_, .right = .none, .pre = true },
    .{ .name = "*", .native_index = 144, .precedence = 16, .ret = .int_, .left = .int_, .right = .int_ },
    .{ .name = "/", .native_index = 145, .precedence = 16, .ret = .int_, .left = .int_, .right = .int_ },
    .{ .name = "%", .native_index = 253, .precedence = 18, .ret = .int_, .left = .int_, .right = .int_ },
    .{ .name = "+", .native_index = 146, .precedence = 20, .ret = .int_, .left = .int_, .right = .int_ },
    .{ .name = "-", .native_index = 147, .precedence = 20, .ret = .int_, .left = .int_, .right = .int_ },
    .{ .name = "<<", .native_index = 148, .precedence = 22, .ret = .int_, .left = .int_, .right = .int_ },
    .{ .name = ">>", .native_index = 149, .precedence = 22, .ret = .int_, .left = .int_, .right = .int_ },
    .{ .name = ">>>", .native_index = 196, .precedence = 22, .ret = .int_, .left = .int_, .right = .int_ },
    .{ .name = "<", .native_index = 150, .precedence = 24, .ret = .bool_, .left = .int_, .right = .int_ },
    .{ .name = ">", .native_index = 151, .precedence = 24, .ret = .bool_, .left = .int_, .right = .int_ },
    .{ .name = "<=", .native_index = 152, .precedence = 24, .ret = .bool_, .left = .int_, .right = .int_ },
    .{ .name = ">=", .native_index = 153, .precedence = 24, .ret = .bool_, .left = .int_, .right = .int_ },
    .{ .name = "==", .native_index = 154, .precedence = 24, .ret = .bool_, .left = .int_, .right = .int_ },
    .{ .name = "!=", .native_index = 155, .precedence = 26, .ret = .bool_, .left = .int_, .right = .int_ },
    .{ .name = "&", .native_index = 156, .precedence = 28, .ret = .int_, .left = .int_, .right = .int_ },
    .{ .name = "^", .native_index = 157, .precedence = 28, .ret = .int_, .left = .int_, .right = .int_ },
    .{ .name = "|", .native_index = 158, .precedence = 28, .ret = .int_, .left = .int_, .right = .int_ },
    .{ .name = "++", .native_index = 163, .precedence = 0, .ret = .int_, .left = .int_, .right = .none, .pre = true, .out = true },
    .{ .name = "--", .native_index = 164, .precedence = 0, .ret = .int_, .left = .int_, .right = .none, .pre = true, .out = true },
    .{ .name = "++", .native_index = 165, .precedence = 0, .ret = .int_, .left = .int_, .right = .none, .post = true, .out = true },
    .{ .name = "--", .native_index = 166, .precedence = 0, .ret = .int_, .left = .int_, .right = .none, .post = true, .out = true },

    // Float
    .{ .name = "-", .native_index = 169, .precedence = 0, .ret = .float_, .left = .float_, .right = .none, .pre = true },
    .{ .name = "**", .native_index = 170, .precedence = 12, .ret = .float_, .left = .float_, .right = .float_ },
    .{ .name = "*", .native_index = 171, .precedence = 16, .ret = .float_, .left = .float_, .right = .float_ },
    .{ .name = "/", .native_index = 172, .precedence = 16, .ret = .float_, .left = .float_, .right = .float_ },
    .{ .name = "%", .native_index = 173, .precedence = 18, .ret = .float_, .left = .float_, .right = .float_ },
    .{ .name = "+", .native_index = 174, .precedence = 20, .ret = .float_, .left = .float_, .right = .float_ },
    .{ .name = "-", .native_index = 175, .precedence = 20, .ret = .float_, .left = .float_, .right = .float_ },
    .{ .name = "<", .native_index = 176, .precedence = 24, .ret = .bool_, .left = .float_, .right = .float_ },
    .{ .name = ">", .native_index = 177, .precedence = 24, .ret = .bool_, .left = .float_, .right = .float_ },
    .{ .name = "<=", .native_index = 178, .precedence = 24, .ret = .bool_, .left = .float_, .right = .float_ },
    .{ .name = ">=", .native_index = 179, .precedence = 24, .ret = .bool_, .left = .float_, .right = .float_ },
    .{ .name = "==", .native_index = 180, .precedence = 24, .ret = .bool_, .left = .float_, .right = .float_ },
    .{ .name = "~=", .native_index = 210, .precedence = 24, .ret = .bool_, .left = .float_, .right = .float_ },
    .{ .name = "!=", .native_index = 181, .precedence = 26, .ret = .bool_, .left = .float_, .right = .float_ },

    // String
    .{ .name = "$", .native_index = 112, .precedence = 40, .ret = .string, .left = .string, .right = .string, .coerce = true },
    .{ .name = "@", .native_index = 168, .precedence = 40, .ret = .string, .left = .string, .right = .string, .coerce = true },
    .{ .name = "<", .native_index = 115, .precedence = 24, .ret = .bool_, .left = .string, .right = .string },
    .{ .name = ">", .native_index = 116, .precedence = 24, .ret = .bool_, .left = .string, .right = .string },
    .{ .name = "<=", .native_index = 120, .precedence = 24, .ret = .bool_, .left = .string, .right = .string },
    .{ .name = ">=", .native_index = 121, .precedence = 24, .ret = .bool_, .left = .string, .right = .string },
    .{ .name = "==", .native_index = 122, .precedence = 24, .ret = .bool_, .left = .string, .right = .string },
    .{ .name = "!=", .native_index = 123, .precedence = 26, .ret = .bool_, .left = .string, .right = .string },
    .{ .name = "~=", .native_index = 124, .precedence = 24, .ret = .bool_, .left = .string, .right = .string },

    // Object
    .{ .name = "==", .native_index = 114, .precedence = 24, .ret = .bool_, .left = .object_reference, .right = .object_reference },
    .{ .name = "!=", .native_index = 119, .precedence = 26, .ret = .bool_, .left = .object_reference, .right = .object_reference },

    // Name
    .{ .name = "==", .native_index = 254, .precedence = 24, .ret = .bool_, .left = .name, .right = .name },
    .{ .name = "!=", .native_index = 255, .precedence = 26, .ret = .bool_, .left = .name, .right = .name },

    // Vector
    .{ .name = "-", .native_index = 211, .precedence = 0, .ret = .struct_, .left = .struct_, .right = .none, .pre = true, .left_struct = "Vector" },
    .{ .name = "*", .native_index = 212, .precedence = 16, .ret = .struct_, .left = .struct_, .right = .float_, .left_struct = "Vector" },
    .{ .name = "*", .native_index = 213, .precedence = 16, .ret = .struct_, .left = .float_, .right = .struct_, .right_struct = "Vector" },
    .{ .name = "/", .native_index = 214, .precedence = 16, .ret = .struct_, .left = .struct_, .right = .float_, .left_struct = "Vector" },
    .{ .name = "+", .native_index = 215, .precedence = 20, .ret = .struct_, .left = .struct_, .right = .struct_, .left_struct = "Vector", .right_struct = "Vector" },
    .{ .name = "-", .native_index = 216, .precedence = 20, .ret = .struct_, .left = .struct_, .right = .struct_, .left_struct = "Vector", .right_struct = "Vector" },
    .{ .name = "==", .native_index = 217, .precedence = 24, .ret = .bool_, .left = .struct_, .right = .struct_, .left_struct = "Vector", .right_struct = "Vector" },
    .{ .name = "!=", .native_index = 218, .precedence = 26, .ret = .bool_, .left = .struct_, .right = .struct_, .left_struct = "Vector", .right_struct = "Vector" },
    .{ .name = "Dot", .native_index = 219, .precedence = 16, .ret = .float_, .left = .struct_, .right = .struct_, .left_struct = "Vector", .right_struct = "Vector" },
    .{ .name = "Cross", .native_index = 220, .precedence = 16, .ret = .struct_, .left = .struct_, .right = .struct_, .left_struct = "Vector", .right_struct = "Vector" },

    // Rotator
    .{ .name = "==", .native_index = 142, .precedence = 24, .ret = .bool_, .left = .struct_, .right = .struct_, .left_struct = "Rotator", .right_struct = "Rotator" },
    .{ .name = "!=", .native_index = 203, .precedence = 26, .ret = .bool_, .left = .struct_, .right = .struct_, .left_struct = "Rotator", .right_struct = "Rotator" },
    .{ .name = "*", .native_index = 287, .precedence = 16, .ret = .struct_, .left = .struct_, .right = .float_, .left_struct = "Rotator" },
    .{ .name = "*", .native_index = 288, .precedence = 16, .ret = .struct_, .left = .float_, .right = .struct_, .right_struct = "Rotator" },
    .{ .name = "/", .native_index = 289, .precedence = 16, .ret = .struct_, .left = .struct_, .right = .float_, .left_struct = "Rotator" },
    .{ .name = "+", .native_index = 316, .precedence = 20, .ret = .struct_, .left = .struct_, .right = .struct_, .left_struct = "Rotator", .right_struct = "Rotator" },
    .{ .name = "-", .native_index = 317, .precedence = 20, .ret = .struct_, .left = .struct_, .right = .struct_, .left_struct = "Rotator", .right_struct = "Rotator" },
};

/// Relative cost of converting `src` to `dest` for operator resolution.
/// 0 = identical, 1 = widening (safe), 2 = narrowing, maxInt = impossible.
pub fn conversionCost(dest: TypeInfo, src: TypeInfo) i32 {
    if (dest.prop_type == src.prop_type) return 0;
    if (conversion(dest, src) == null) return std.math.maxInt(i32);
    const widening = switch (dest.prop_type) {
        .int_, .float_, .string, .bool_ => switch (src.prop_type) {
            .byte, .int_ => dest.prop_type != .byte,
            else => false,
        },
        else => false,
    };
    return if (widening) 1 else 2;
}

/// Conversion cost between two types. Returns the cast token to use, or null
/// if no conversion is possible.
pub fn conversion(dest: TypeInfo, src: TypeInfo) ?opcodes.Cast {
    if (src.prop_type == dest.prop_type) return null;
    switch (dest.prop_type) {
        .int_ => switch (src.prop_type) {
            .byte => return .byte_to_int,
            .float_ => return .float_to_int,
            .bool_ => return .bool_to_int,
            else => return null,
        },
        .byte => switch (src.prop_type) {
            .int_ => return .int_to_byte,
            .float_ => return .float_to_byte,
            .bool_ => return .bool_to_byte,
            .string => return .string_to_byte,
            else => return null,
        },
        .float_ => switch (src.prop_type) {
            .byte => return .byte_to_float,
            .int_ => return .int_to_float,
            .bool_ => return .bool_to_float,
            .string => return .string_to_float,
            else => return null,
        },
        .bool_ => switch (src.prop_type) {
            .byte => return .byte_to_bool,
            .int_ => return .int_to_bool,
            .float_ => return .float_to_bool,
            .name => return .name_to_bool,
            .string => return .string_to_bool,
            .object_reference => return .object_to_bool,
            .struct_ => {
                if (src.isVector()) return .vector_to_bool;
                if (src.isRotator()) return .rotator_to_bool;
                return null;
            },
            else => return null,
        },
        .string => switch (src.prop_type) {
            .byte => return .byte_to_string,
            .int_ => return .int_to_string,
            .float_ => return .float_to_string,
            .bool_ => return .bool_to_string,
            .name => return .name_to_string,
            .object_reference => return .object_to_string,
            .struct_ => {
                if (src.isVector()) return .vector_to_string;
                if (src.isRotator()) return .rotator_to_string;
                return null;
            },
            else => return null,
        },
        .name => switch (src.prop_type) {
            .string => return .string_to_name,
            else => return null,
        },
        else => return null,
    }
}
