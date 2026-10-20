// The compiled object model: the UClass/UFunction/UProperty/UEnum/UState/
// UScriptStruct objects the compiler produces, which package.zig serializes
// into a UPK.

const std = @import("std");
const opcodes = @import("opcodes.zig");

pub const Kind = enum {
    class_obj,
    function_obj,
    property_obj,
    enum_obj,
    struct_obj,
    state_obj,
    cdo,
};

/// A single enum declaration.
pub const Enum = struct {
    /// 0-based position in the package export map (-1 until assigned).
    export_index: i32 = -1,
    name: []const u8,
    /// Value names, in declaration order (the _MAX sentinel is appended by the
    /// compiler).
    values: []const []const u8,

    pub fn deinit(self: *Enum, allocator: std.mem.Allocator) void {
        allocator.free(self.values);
    }
};

/// A script struct declaration.
pub const ScriptStruct = struct {
    export_index: i32 = -1,
    name: []const u8,
    flags: u32,
    super_name: []const u8,
    fields: std.ArrayList(*Property),
    offset: i32,

    pub fn deinit(self: *ScriptStruct, allocator: std.mem.Allocator) void {
        for (self.fields.items) |p| p.deinit(allocator);
        self.fields.deinit(allocator);
    }
};

/// One property (class member, local, or parameter).
pub const Property = struct {
    export_index: i32 = -1,
    name: []const u8,
    prop_type: opcodes.PropType,
    flags: u64,
    array_dim: i32, // 1 = scalar, 0 = dynamic array, >1 = static array
    array_size_enum: ?*Enum,

    // Type-specific payloads (mutually exclusive by prop_type).
    enum_: ?*Enum, // CPT_Byte with an enum
    property_class: ?[]const u8, // CPT_ObjectReference/Interface: target class name
    meta_class: ?[]const u8, // class<limiter>
    struct_: ?*ScriptStruct, // CPT_Struct
    inner: ?*Property, // dynamic array element
    delegate_function: ?[]const u8, // CPT_Delegate

    // Function parameter bookkeeping.
    is_param: bool,
    is_return: bool,
    is_out: bool,
    is_optional: bool,
    is_coerce: bool,
    is_const: bool,
    is_skip: bool,
    optional_default: ?[]const u8, // raw expression text for an optional param

    /// Offset within the owning struct's property area (assigned at link time).
    offset: i32,
    /// Serialized next-field pointer chain (class fields).
    next: ?*Property,
    /// Layout size in bytes.
    element_size: i32,

    pub fn deinit(self: *Property, allocator: std.mem.Allocator) void {
        if (self.inner) |inner| inner.deinit(allocator);
    }
};

pub const Param = struct {
    prop: *Property,
};

/// A function declaration.
pub const Function = struct {
    export_index: i32 = -1,
    name: []const u8,
    flags: u32,
    export_flags: u32,
    i_native: i32,
    oper_precedence: i32,
    friendly_name: []const u8,
    /// Index of this function in its owner's children chain (-1 = none).
    next: ?*Function,

    params: std.ArrayList(*Property),
    return_prop: ?*Property,
    locals: std.ArrayList(*Property),
    script: std.ArrayList(u8),
    script_bytecode_size: i32,
    script_storage_size: i32,

    /// For states: owner state (null for class-level functions).
    owner_state: ?*State,

    /// Deferred body compilation: token range [body_start, body_end) of the
    /// owning source file that holds the function body (has_body = true).
    has_body: bool = false,
    body_start: usize = 0,
    body_end: usize = 0,

    pub fn deinit(self: *Function, allocator: std.mem.Allocator) void {
        for (self.params.items) |p| p.deinit(allocator);
        for (self.locals.items) |p| p.deinit(allocator);
        self.params.deinit(allocator);
        self.locals.deinit(allocator);
        self.script.deinit(allocator);
    }
};

/// A state declaration.
pub const State = struct {
    export_index: i32 = -1,
    name: []const u8,
    flags: u32,
    label_table_offset: i32,
    probe_mask: u32,
    functions: std.ArrayList(*Function),
    script: std.ArrayList(u8),
    script_bytecode_size: i32,
    script_storage_size: i32,
    /// Labels defined in this state's code.
    labels: std.StringHashMap(i32),

    pub fn deinit(self: *State, allocator: std.mem.Allocator) void {
        for (self.functions.items) |f| f.deinit(allocator);
        self.functions.deinit(allocator);
        self.script.deinit(allocator);
        self.labels.deinit();
    }
};

/// One assignment in a `defaultproperties` block.
pub const DefaultValue = struct {
    name: []const u8,
    value: []const u8,
};

/// A compiled class.
pub const ClassObj = struct {
    export_index: i32 = -1,
    /// CDO package index (assigned by the package builder).
    cdo_index: i32 = -1,
    name: []const u8,
    package_name: []const u8,
    super_name: []const u8,
    within_name: []const u8,
    config_name: []const u8,
    class_flags: u32,

    fields: std.ArrayList(*Property),
    functions: std.ArrayList(*Function),
    states: std.ArrayList(*State),
    enums: std.ArrayList(*Enum),
    structs: std.ArrayList(*ScriptStruct),

    /// defaultproperties: ordered list of (name, value text).
    defaults: std.ArrayList(DefaultValue),

    /// Layout offset (total property size).
    properties_size: i32,

    pub fn findEnum(self: *ClassObj, name: []const u8) ?*Enum {
        for (self.enums.items) |e| {
            if (std.mem.eql(u8, e.name, name)) return e;
        }
        return null;
    }

    pub fn findStruct(self: *ClassObj, name: []const u8) ?*ScriptStruct {
        for (self.structs.items) |s| {
            if (std.mem.eql(u8, s.name, name)) return s;
        }
        return null;
    }

    pub fn findProperty(self: *ClassObj, name: []const u8) ?*Property {
        for (self.fields.items) |f| {
            if (std.mem.eql(u8, f.name, name)) return f;
        }
        return null;
    }

    pub fn findFunction(self: *ClassObj, name: []const u8) ?*Function {
        for (self.functions.items) |f| {
            if (std.mem.eql(u8, f.name, name)) return f;
        }
        for (self.states.items) |s| {
            for (s.functions.items) |f| {
                if (std.mem.eql(u8, f.name, name)) return f;
            }
        }
        return null;
    }

    pub fn deinit(self: *ClassObj, allocator: std.mem.Allocator) void {
        for (self.fields.items) |f| f.deinit(allocator);
        for (self.functions.items) |f| f.deinit(allocator);
        for (self.states.items) |s| s.deinit(allocator);
        for (self.enums.items) |e| e.deinit(allocator);
        for (self.structs.items) |s| s.deinit(allocator);
        self.fields.deinit(allocator);
        self.functions.deinit(allocator);
        self.states.deinit(allocator);
        self.enums.deinit(allocator);
        self.structs.deinit(allocator);
        self.defaults.deinit(allocator);
    }
};
