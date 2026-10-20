// Serialize the compiled object model into a UPK file. Follows the reference
// engine's Serialize functions (Core/Src/UnClass.cpp, UnProp.cpp, UnObj.cpp):
// every export body starts with its NetIndex, and non-UClass fields carry a
// ScriptProps "None" terminator.

const std = @import("std");

const opcodes = @import("opcodes.zig");
const model = @import("model.zig");
const compile = @import("compile.zig");
const unreal = @import("unreal");

const Compiler = compile.Compiler;
const CompileError = compile.CompileError;

pub const WriteError = error{
    InvalidExportIndex,
    OutOfMemory,
} || std.mem.Allocator.Error;

const Byte = struct {
    data: []u8,
    deinit: bool = true,
};

/// One collected export, ordered by export_index.
const Export = struct {
    name: []const u8,
    name_number: i32 = 0,
    class_ref: i32,
    super_ref: i32,
    outer_ref: i32,
    flags: u64,
    body: []u8,
    serial_offset: i32 = 0,
    serial_size: i32 = 0,
    archetype_ref: i32 = 0,

    fn deinit(self: *Export, allocator: std.mem.Allocator) void {
        allocator.free(self.body);
    }
};

fn wr(w: *std.ArrayList(u8), a: std.mem.Allocator, bytes: []const u8) !void {
    try w.appendSlice(a, bytes);
}

fn wrInt(w: *std.ArrayList(u8), a: std.mem.Allocator, v: i32) !void {
    var buf: [4]u8 = undefined;
    std.mem.writeInt(i32, &buf, v, .little);
    try w.appendSlice(a, &buf);
}

fn wrU32(w: *std.ArrayList(u8), a: std.mem.Allocator, v: u32) !void {
    try wrInt(w, a, @bitCast(v));
}

fn wrU64(w: *std.ArrayList(u8), a: std.mem.Allocator, v: u64) !void {
    var buf: [8]u8 = undefined;
    std.mem.writeInt(u64, &buf, v, .little);
    try w.appendSlice(a, &buf);
}

fn wrWord(w: *std.ArrayList(u8), a: std.mem.Allocator, v: u16) !void {
    var buf: [2]u8 = undefined;
    std.mem.writeInt(u16, &buf, v, .little);
    try w.appendSlice(a, &buf);
}

/// Write an FName (name-map index, instance number).
fn wrName(w: *std.ArrayList(u8), a: std.mem.Allocator, c: *Compiler, name: []const u8, number: i32) !void {
    const idx = try c.nameIndex(name);
    try wrInt(w, a, @intCast(idx));
    try wrInt(w, a, number);
}

/// Write a length-prefixed UE3 string (positive = ANSI).
fn wrString(w: *std.ArrayList(u8), a: std.mem.Allocator, s: []const u8) !void {
    var wide = false;
    for (s) |b| {
        if (b >= 0x80) {
            wide = true;
            break;
        }
    }
    if (wide) {
        const units = try std.unicode.utf8ToUtf16LeAlloc(a, s);
        defer a.free(units);
        try wrInt(w, a, -@as(i32, @intCast(units.len + 1)));
        for (units) |u| {
            var buf: [2]u8 = undefined;
            std.mem.writeInt(u16, &buf, u, .little);
            try w.appendSlice(a, &buf);
        }
        var zero: [2]u8 = .{ 0, 0 };
        try w.appendSlice(a, &zero);
    } else {
        try wrInt(w, a, @intCast(s.len + 1));
        try w.appendSlice(a, s);
        try w.append(a, 0);
    }
}

/// Write a TArray count (empty arrays are just a zero count).
fn wrEmptyArray(w: *std.ArrayList(u8), a: std.mem.Allocator) !void {
    try wrInt(w, a, 0);
}

fn propRef(p: *model.Property) i32 {
    return p.export_index + 1;
}

fn funcRef(f: *model.Function) i32 {
    return f.export_index + 1;
}

fn objRef(export_index: i32) i32 {
    return export_index + 1;
}

/// The package index of an import entry (already negative).
fn importRef(c: *Compiler, class_name: []const u8) !i32 {
    return c.importCore(class_name);
}

/// Serialize the tagged-property defaults of a class (its defaultproperties).
fn writeDefaults(
    c: *Compiler,
    cls: *model.ClassObj,
    w: *std.ArrayList(u8),
    a: std.mem.Allocator,
) !void {
    // The tagged-property stream: for now, a single `None` terminator (no
    // property values are serialized yet).
    try wrName(w, a, c, "None", 0);
    _ = cls;
}

/// Write a property's body and recurse into its array inner.
fn writePropertyBodyRecursive(
    c: *Compiler,
    exports: []Export,
    p: *model.Property,
    outer_ref: i32,
    next_ref: i32,
    a: std.mem.Allocator,
) !void {
    var body = std.ArrayList(u8).empty;
    defer body.deinit(a);
    try writePropertyBody(c, p, outer_ref, next_ref, &body, a);
    const owned = try body.toOwnedSlice(a);
    const e = &exports[@intCast(p.export_index)];
    if (e.body.len > 0) a.free(e.body);
    e.body = owned;
    if (p.inner) |inner| {
        try writePropertyBodyRecursive(c, exports, inner, outer_ref, 0, a);
    }
}

/// Serialize one UProperty export body.
fn writePropertyBody(
    c: *Compiler,
    p: *model.Property,
    outer_ref: i32,
    next_ref: i32,
    w: *std.ArrayList(u8),
    a: std.mem.Allocator,
) !void {
    // UObject::Serialize: NetIndex + ScriptProps.
    try wrInt(w, a, propRef(p)); // NetIndex
    try wrName(w, a, c, "None", 0); // ScriptProps terminator

    // UField::Serialize: Next.
    try wrInt(w, a, next_ref);

    // UProperty::Serialize.
    try wrInt(w, a, p.array_dim);
    try wrU64(w, a, p.flags);
    try wrName(w, a, c, "None", 0); // Category
    // ArraySizeEnum is a UEnum* object reference (4 bytes), 0 when absent.
    try wrInt(w, a, 0);
    if (p.flags & opcodes.cp.net != 0) try wrInt(w, a, 0); // RepOffset

    // Dynamic array Inner reference (UArrayProperty::Serialize).
    if (p.array_dim == 0) {
        if (p.inner) |inner| {
            try wrInt(w, a, propRef(inner));
        } else {
            try wrInt(w, a, 0);
        }
    }

    // Subclass data.
    switch (p.prop_type) {
        .byte => {
            // UByteProperty::Serialize: Enum.
            if (p.enum_) |e| {
                try wrInt(w, a, objRef(e.export_index));
            } else {
                try wrInt(w, a, 0);
            }
        },
        .object_reference, .interface => {
            // UObjectProperty::Serialize: PropertyClass.
            if (p.property_class) |cn| {
                if (c.findClass(cn)) |pc| {
                    try wrInt(w, a, objRef(pc.export_index));
                } else {
                    try wrInt(w, a, try importRef(c, cn));
                }
            } else {
                try wrInt(w, a, 0);
            }
        },
        .struct_ => {
            if (p.struct_) |s| {
                if (s.export_index >= 0) {
                    try wrInt(w, a, objRef(s.export_index));
                } else {
                    try wrInt(w, a, 0); // builtin Vector/Rotator
                }
            } else {
                try wrInt(w, a, 0);
            }
        },
        .delegate => {
            // UDelegateProperty::Serialize: Function + SourceDelegate.
            try wrInt(w, a, 0);
            try wrInt(w, a, 0);
        },
        else => {},
    }
    _ = outer_ref;
}

/// Serialize one UFunction export body.
fn writeFunctionBody(
    c: *Compiler,
    f: *model.Function,
    next_ref: i32,
    children_ref: i32,
    w: *std.ArrayList(u8),
    a: std.mem.Allocator,
) !void {
    try wrInt(w, a, funcRef(f)); // NetIndex
    try wrName(w, a, c, "None", 0); // ScriptProps

    // UField::Serialize: Next.
    try wrInt(w, a, next_ref);
    // UStruct::Serialize (uncooked: ScriptText + Children + CppText + Line + TextPos).
    try wrInt(w, a, 0); // SuperStruct (functions have none)
    try wrInt(w, a, 0); // ScriptText (UTextBuffer*)
    try wrInt(w, a, children_ref); // Children (first param)
    try wrInt(w, a, 0); // CppText (UTextBuffer*)
    try wrInt(w, a, -1); // Line
    try wrInt(w, a, -1); // TextPos
    try wrInt(w, a, f.script_bytecode_size);
    try wrInt(w, a, f.script_storage_size);
    try w.appendSlice(a, f.script.items);

    // UFunction::Serialize.
    try wrInt(w, a, f.i_native);
    try wrInt(w, a, f.oper_precedence);
    try wrU32(w, a, f.flags);
    if (f.flags & opcodes.func.net != 0) try wrInt(w, a, 0); // RepOffset
    try wrName(w, a, c, f.friendly_name, 0); // FriendlyName
}

/// Serialize one UClass export body.
fn writeClassBody(
    c: *Compiler,
    cls: *model.ClassObj,
    super_ref: i32,
    children_ref: i32,
    cdo_ref: i32,
    w: *std.ArrayList(u8),
    a: std.mem.Allocator,
) !void {
    try wrInt(w, a, objRef(cls.export_index)); // NetIndex
    // UField::Serialize: Next (top-level class: none).
    try wrInt(w, a, 0);
    // UStruct::Serialize.
    try wrInt(w, a, super_ref); // SuperStruct
    try wrInt(w, a, 0); // ScriptText (UTextBuffer* object ref, none for script classes)
    try wrInt(w, a, children_ref); // Children (first field)
    try wrInt(w, a, 0); // CppText (UTextBuffer*)
    try wrInt(w, a, -1); // Line
    try wrInt(w, a, -1); // TextPos
    try wrInt(w, a, 0); // ScriptBytecodeSize
    try wrInt(w, a, 0); // ScriptStorageSize

    // UClass::Serialize.
    try wrU32(w, a, cls.class_flags);
    const within_ref = if (c.findClass(cls.within_name)) |wcls|
        objRef(wcls.export_index)
    else
        try importRef(c, "Object");
    try wrInt(w, a, within_ref); // ClassWithin
    try wrName(w, a, c, if (cls.config_name.len > 0) cls.config_name else "None", 0); // ClassConfigName
    try wrEmptyArray(w, a); // ComponentNameToDefaultObjectMap
    try wrEmptyArray(w, a); // Interfaces
    try wrEmptyArray(w, a); // DontSortCategories
    try wrEmptyArray(w, a); // HideCategories
    try wrEmptyArray(w, a); // AutoExpandCategories
    try wrEmptyArray(w, a); // AutoCollapseCategories
    try wrInt(w, a, 0); // bForceScriptOrder
    try wrEmptyArray(w, a); // ClassGroupNames
    try wrString(w, a, ""); // ClassHeaderFilename
    try wrName(w, a, c, "None", 0); // DLLBindName
    try wrInt(w, a, cdo_ref); // ClassDefaultObject
}

/// Serialize one UEnum export body.
fn writeEnumBody(
    c: *Compiler,
    e: *model.Enum,
    next_ref: i32,
    w: *std.ArrayList(u8),
    a: std.mem.Allocator,
) !void {
    try wrInt(w, a, objRef(e.export_index)); // NetIndex
    try wrName(w, a, c, "None", 0); // ScriptProps
    try wrInt(w, a, next_ref); // Next
    // UEnum::Serialize: Names (TArray<FName>).
    try wrInt(w, a, @intCast(e.values.len));
    for (e.values) |v| {
        try wrName(w, a, c, v, 0);
    }
}

/// Serialize one UScriptStruct export body.
fn writeStructBody(
    c: *Compiler,
    s: *model.ScriptStruct,
    next_ref: i32,
    children_ref: i32,
    w: *std.ArrayList(u8),
    a: std.mem.Allocator,
) !void {
    try wrInt(w, a, objRef(s.export_index)); // NetIndex
    try wrName(w, a, c, "None", 0); // ScriptProps
    try wrInt(w, a, next_ref); // Next
    try wrInt(w, a, 0); // SuperStruct
    try wrInt(w, a, 0); // ScriptText (UTextBuffer*)
    try wrInt(w, a, children_ref); // Children
    try wrInt(w, a, 0); // CppText (UTextBuffer*)
    try wrInt(w, a, -1); // Line
    try wrInt(w, a, -1); // TextPos
    try wrInt(w, a, 0); // ScriptBytecodeSize
    try wrInt(w, a, 0); // ScriptStorageSize
    // UScriptStruct::Serialize.
    try wrU32(w, a, s.flags);
    try wrName(w, a, c, "None", 0); // empty tagged defaults
}

/// Serialize one UState export body.
fn writeStateBody(
    c: *Compiler,
    st: *model.State,
    next_ref: i32,
    children_ref: i32,
    w: *std.ArrayList(u8),
    a: std.mem.Allocator,
) !void {
    try wrInt(w, a, objRef(st.export_index)); // NetIndex
    try wrName(w, a, c, "None", 0); // ScriptProps
    try wrInt(w, a, next_ref); // Next
    try wrInt(w, a, 0); // SuperStruct
    try wrInt(w, a, 0); // ScriptText (UTextBuffer*)
    try wrInt(w, a, children_ref); // Children
    try wrInt(w, a, 0); // CppText (UTextBuffer*)
    try wrInt(w, a, -1); // Line
    try wrInt(w, a, -1); // TextPos
    try wrInt(w, a, st.script_bytecode_size);
    try wrInt(w, a, st.script_storage_size);
    try w.appendSlice(a, st.script.items);
    // UState::Serialize.
    try wrU32(w, a, st.probe_mask);
    try wrInt(w, a, st.label_table_offset);
    try wrU32(w, a, st.flags);
    try wrEmptyArray(w, a); // FuncMap
}

/// Collect all exports from the compiled classes, in export_index order.
fn collectExports(
    c: *Compiler,
    a: std.mem.Allocator,
) !struct { exports: []Export, count: i32 } {
    // Reserve CDO indices first so the export array is the right size.
    for (c.classes.items) |cls| {
        if (cls.cdo_index < 0) cls.cdo_index = c.nextExport();
    }
    const count: i32 = c.next_export;
    const exports = try a.alloc(Export, @intCast(count));
    for (exports) |*e| e.* = .{
        .name = "",
        .class_ref = 0,
        .super_ref = 0,
        .outer_ref = 0,
        .flags = 0,
        .body = &.{},
    };

    for (c.classes.items) |cls| {
        const outer = objRef(cls.export_index);

        // Class fields.
        for (cls.fields.items) |f| {
            try collectPropertyExports(c, exports, f, outer);
        }
        // Enums.
        for (cls.enums.items) |e| {
            exports[@intCast(e.export_index)] = .{
                .name = e.name,
                .class_ref = try importRef(c, "Enum"),
                .super_ref = 0,
                .outer_ref = outer,
                .flags = opcodes.rf.public_,
                .body = &.{},
            };
        }
        // Structs and their member properties.
        for (cls.structs.items) |s| {
            const struct_outer = objRef(s.export_index);
            for (s.fields.items) |f| {
                try collectPropertyExports(c, exports, f, struct_outer);
            }
            exports[@intCast(s.export_index)] = .{
                .name = s.name,
                .class_ref = try importRef(c, "ScriptStruct"),
                .super_ref = 0,
                .outer_ref = outer,
                .flags = opcodes.rf.public_,
                .body = &.{},
            };
        }
        // Functions, their params/return/locals.
        for (cls.functions.items) |f| {
            try collectFunctionProps(c, exports, f);
            exports[@intCast(f.export_index)] = .{
                .name = f.name,
                .class_ref = try importRef(c, "Function"),
                .super_ref = 0,
                .outer_ref = outer,
                .flags = opcodes.rf.public_,
                .body = &.{},
            };
        }
        // States and their functions.
        for (cls.states.items) |st| {
            const state_outer = objRef(st.export_index);
            for (st.functions.items) |f| {
                try collectFunctionProps(c, exports, f);
                exports[@intCast(f.export_index)] = .{
                    .name = f.name,
                    .class_ref = try importRef(c, "Function"),
                    .super_ref = 0,
                    .outer_ref = state_outer,
                    .flags = opcodes.rf.public_,
                    .body = &.{},
                };
            }
            exports[@intCast(st.export_index)] = .{
                .name = st.name,
                .class_ref = try importRef(c, "State"),
                .super_ref = 0,
                .outer_ref = outer,
                .flags = opcodes.rf.public_,
                .body = &.{},
            };
        }
        // The class itself.
        exports[@intCast(cls.export_index)] = .{
            .name = cls.name,
            .class_ref = try importRef(c, "Class"),
            .super_ref = if (c.findClass(cls.super_name)) |sc| objRef(sc.export_index) else 0,
            .outer_ref = 0,
            .flags = opcodes.rf.public_ | opcodes.rf.standalone,
            .body = &.{},
        };
        // The CDO.
        exports[@intCast(cls.cdo_index)] = .{
            .name = try std.fmt.allocPrint(a, "Default__{s}", .{cls.name}),
            .class_ref = objRef(cls.export_index),
            .super_ref = 0,
            .outer_ref = 0,
            .flags = opcodes.rf.public_ | opcodes.rf.class_default_object,
            .body = &.{},
        };
    }

    // Register every export's name before the name map is serialized.
    for (exports) |e| {
        _ = try c.nameIndex(e.name);
    }
    return .{ .exports = exports, .count = count };
}

/// Create export entries for a function's params, return value, and locals.
fn collectFunctionProps(c: *Compiler, exports: []Export, f: *model.Function) !void {
    const func_outer = funcRef(f);
    for (f.params.items) |p| {
        try collectPropertyExports(c, exports, p, func_outer);
    }
    if (f.return_prop) |rp| {
        try collectPropertyExports(c, exports, rp, func_outer);
    }
    for (f.locals.items) |l| {
        try collectPropertyExports(c, exports, l, func_outer);
    }
}

/// Create an export entry for a property and recursively for its array inner.
fn collectPropertyExports(c: *Compiler, exports: []Export, p: *model.Property, outer_ref: i32) !void {
    exports[@intCast(p.export_index)] = .{
        .name = p.name,
        .class_ref = try propertyClassRef(c, p),
        .super_ref = 0,
        .outer_ref = outer_ref,
        .flags = opcodes.rf.public_,
        .body = &.{},
    };
    if (p.inner) |inner| {
        try collectPropertyExports(c, exports, inner, outer_ref);
    }
}

fn propertyClassRef(c: *Compiler, p: *model.Property) !i32 {
    const name = switch (p.prop_type) {
        .byte => "ByteProperty",
        .int_ => "IntProperty",
        .bool_ => "BoolProperty",
        .float_ => "FloatProperty",
        .string => "StrProperty",
        .name => "NameProperty",
        .object_reference => if (p.property_class != null and std.mem.eql(u8, p.property_class.?, "Class")) "ClassProperty" else "ObjectProperty",
        .interface => "InterfaceProperty",
        .delegate => "DelegateProperty",
        .struct_ => "StructProperty",
        .map => "MapProperty",
        .none => "ObjectProperty",
        .range, .vector, .rotation => "StructProperty",
    };
    return c.importPropertyClass(name);
}

/// Build the full package file bytes.
pub fn buildPackage(c: *Compiler, a: std.mem.Allocator) ![]u8 {
    // Collect exports (this assigns CDO indices).
    const collected = try collectExports(c, a);
    const exports = collected.exports;
    const export_count: usize = @intCast(collected.count);
    defer a.free(exports);

    // Serialize each export body.
    for (c.classes.items) |cls| {
        const outer = objRef(cls.export_index);

        // Fields: next = previous in chain, children = last field.
        var prev_field_ref: i32 = 0;
        var children_ref: i32 = 0;
        for (cls.fields.items) |f| {
            try writePropertyBodyRecursive(c, exports, f, outer, prev_field_ref, a);
            children_ref = propRef(f);
            prev_field_ref = propRef(f);
        }

        // Enums.
        for (cls.enums.items) |e| {
            var body = std.ArrayList(u8).empty;
            defer body.deinit(a);
            try writeEnumBody(c, e, 0, &body, a);
            const owned = try body.toOwnedSlice(a);
            const exp = &exports[@intCast(e.export_index)];
            if (exp.body.len > 0) a.free(exp.body);
            exp.body = owned;
        }

        // Structs.
        for (cls.structs.items) |s| {
            var struct_children: i32 = 0;
            var s_prev: i32 = 0;
            for (s.fields.items) |f| {
                try writePropertyBodyRecursive(c, exports, f, objRef(s.export_index), s_prev, a);
                struct_children = propRef(f);
                s_prev = propRef(f);
            }
            var body = std.ArrayList(u8).empty;
            defer body.deinit(a);
            try writeStructBody(c, s, 0, struct_children, &body, a);
            const owned = try body.toOwnedSlice(a);
            const exp = &exports[@intCast(s.export_index)];
            if (exp.body.len > 0) a.free(exp.body);
            exp.body = owned;
        }

        // Functions.
        var func_children: i32 = 0;
        var prev_func_ref: i32 = 0;
        for (cls.functions.items) |f| {
            // Children = first param (in declaration order).
            var param_children: i32 = 0;
            for (f.params.items) |p| {
                try writePropertyBodyRecursive(c, exports, p, funcRef(f), 0, a);
                param_children = propRef(p);
            }
            if (f.return_prop) |rp| {
                try writePropertyBodyRecursive(c, exports, rp, funcRef(f), 0, a);
                if (param_children == 0) param_children = propRef(rp);
            }
            for (f.locals.items) |l| {
                try writePropertyBodyRecursive(c, exports, l, funcRef(f), 0, a);
            }

            var body = std.ArrayList(u8).empty;
            defer body.deinit(a);
            try writeFunctionBody(c, f, prev_func_ref, param_children, &body, a);
            const owned = try body.toOwnedSlice(a);
            const exp = &exports[@intCast(f.export_index)];
            if (exp.body.len > 0) a.free(exp.body);
            exp.body = owned;
            func_children = funcRef(f);
            prev_func_ref = funcRef(f);
        }

        // States.
        for (cls.states.items) |st| {
            var state_children: i32 = 0;
            var st_prev: i32 = 0;
            for (st.functions.items) |f| {
                var param_children: i32 = 0;
                for (f.params.items) |p| {
                    try writePropertyBodyRecursive(c, exports, p, funcRef(f), 0, a);
                    param_children = propRef(p);
                }
                var body = std.ArrayList(u8).empty;
                defer body.deinit(a);
                try writeFunctionBody(c, f, st_prev, param_children, &body, a);
                const owned = try body.toOwnedSlice(a);
                const exp = &exports[@intCast(f.export_index)];
                if (exp.body.len > 0) a.free(exp.body);
                exp.body = owned;
                state_children = funcRef(f);
                st_prev = funcRef(f);
            }
            var body = std.ArrayList(u8).empty;
            defer body.deinit(a);
            try writeStateBody(c, st, 0, state_children, &body, a);
            const owned = try body.toOwnedSlice(a);
            const exp = &exports[@intCast(st.export_index)];
            if (exp.body.len > 0) a.free(exp.body);
            exp.body = owned;
        }

        // Class body.
        {
            var body = std.ArrayList(u8).empty;
            defer body.deinit(a);
            const super_ref: i32 = if (c.findClass(cls.super_name)) |sc| objRef(sc.export_index) else 0;
            try writeClassBody(c, cls, super_ref, children_ref, objRef(cls.cdo_index), &body, a);
            const owned = try body.toOwnedSlice(a);
            const exp = &exports[@intCast(cls.export_index)];
            if (exp.body.len > 0) a.free(exp.body);
            exp.body = owned;
        }

        // CDO body.
        {
            var body = std.ArrayList(u8).empty;
            defer body.deinit(a);
            try wrInt(&body, a, objRef(cls.cdo_index)); // NetIndex
            try writeDefaults(c, cls, &body, a);
            const owned = try body.toOwnedSlice(a);
            const exp = &exports[@intCast(cls.cdo_index)];
            if (exp.body.len > 0) a.free(exp.body);
            exp.body = owned;
        }
    }

    // Build the name map.
    var name_buf = std.ArrayList(u8).empty;
    defer name_buf.deinit(a);
    for (c.names.items) |n| {
        try wrString(&name_buf, a, n);
        try wrU64(&name_buf, a, 0); // NameEntry flags
    }

    // Build the import map.
    var import_buf = std.ArrayList(u8).empty;
    defer import_buf.deinit(a);
    for (c.imports.items, 0..) |imp, i| {
        const imp_idx: i32 = -@as(i32, @intCast(i + 1));
        try wrName(&import_buf, a, c, imp.class_package, 0);
        try wrName(&import_buf, a, c, imp.class_name, 0);
        try wrInt(&import_buf, a, imp.outer_index);
        try wrName(&import_buf, a, c, imp.object_name, 0);
        _ = imp_idx;
    }

    // Build the export map.
    var export_buf = std.ArrayList(u8).empty;
    defer export_buf.deinit(a);
    // Compute serial offsets first.
    // Serialize the export map entries first (serial sizes are known; the
    // absolute offsets are patched once the header size is computed).
    var cursor: i32 = 0;
    for (exports) |*e| {
        e.serial_size = @intCast(e.body.len);
    }
    for (exports) |*e| {
        try wrInt(&export_buf, a, e.class_ref);
        try wrInt(&export_buf, a, e.super_ref);
        try wrInt(&export_buf, a, e.outer_ref);
        try wrName(&export_buf, a, c, e.name, e.name_number);
        try wrInt(&export_buf, a, e.archetype_ref);
        try wrU64(&export_buf, a, e.flags);
        try wrInt(&export_buf, a, e.serial_size);
        try wrInt(&export_buf, a, e.serial_offset);
        try wrU32(&export_buf, a, 0); // export_flags
        try wrInt(&export_buf, a, 0); // generation_net_object_count count
        try wrInt(&export_buf, a, 0); // package_guid.a
        try wrInt(&export_buf, a, 0);
        try wrInt(&export_buf, a, 0);
        try wrInt(&export_buf, a, 0);
        try wrU32(&export_buf, a, 0); // package_flags
    }
    _ = &cursor;

    // Serialize the summary.
    var summary = unreal.PackageFileSummary{
        .tag = unreal.package_file_tag,
        .file_version = 893,
        .total_header_size = 0,
        .folder_name = .{ .data = c.package_name },
        .package_flags = .{ .cooked = false },
        .name_count = @intCast(c.names.items.len),
        .name_offset = 0,
        .export_count = @intCast(export_count),
        .export_offset = 0,
        .import_count = @intCast(c.imports.items.len),
        .import_offset = 0,
        .depends_offset = 0,
        .import_export_guids_offset = -1,
        .import_guids_count = 0,
        .export_guids_count = 0,
        .thumbnail_table_offset = 0,
        .guid = .{ .a = 0, .b = 0, .c = 0, .d = 0 },
        .generations = &.{},
        .engine_version = 10897,
        .cooked_content_version = 0,
        .compression_flags = .{},
        .compressed_chunks = &.{},
        .package_source = 0,
        .additional_packages_to_cook = &.{},
        .texture_allocations = &.{},
    };
    var summary_buf: std.Io.Writer.Allocating = .init(a);
    defer summary_buf.deinit();
    try summary.write(&summary_buf.writer, a);

    // The export data region begins after the whole header. Serial offsets are
    // absolute file offsets, so shift them by the header size.
    const name_offset: i32 = @intCast(summary_buf.writer.end);
    const import_offset = name_offset + @as(i32, @intCast(name_buf.items.len));
    const export_offset = import_offset + @as(i32, @intCast(import_buf.items.len));
    const total_header_size = export_offset + @as(i32, @intCast(export_buf.items.len));

    // Update the summary with the real offsets and re-serialize.
    summary.total_header_size = total_header_size;
    summary.name_offset = name_offset;
    summary.import_offset = import_offset;
    summary.export_offset = export_offset;

    // Patch the serial offsets in the export map to be absolute.
    var so_cursor: i32 = total_header_size;
    for (exports) |*e| {
        e.serial_offset = so_cursor;
        so_cursor += e.serial_size;
    }
    // Re-write the export map with absolute offsets: easiest to rebuild it.
    export_buf.clearRetainingCapacity();
    for (exports) |*e| {
        try wrInt(&export_buf, a, e.class_ref);
        try wrInt(&export_buf, a, e.super_ref);
        try wrInt(&export_buf, a, e.outer_ref);
        try wrName(&export_buf, a, c, e.name, e.name_number);
        try wrInt(&export_buf, a, e.archetype_ref);
        try wrU64(&export_buf, a, e.flags);
        try wrInt(&export_buf, a, e.serial_size);
        try wrInt(&export_buf, a, e.serial_offset);
        try wrU32(&export_buf, a, 0);
        try wrInt(&export_buf, a, 0);
        try wrInt(&export_buf, a, 0);
        try wrInt(&export_buf, a, 0);
        try wrInt(&export_buf, a, 0);
        try wrInt(&export_buf, a, 0);
        try wrU32(&export_buf, a, 0);
    }

    var out: std.Io.Writer.Allocating = .init(a);
    defer out.deinit();
    try summary.write(&out.writer, a);
    try out.writer.writeAll(name_buf.items);
    try out.writer.writeAll(import_buf.items);
    try out.writer.writeAll(export_buf.items);

    // Append the export data (bodies) in order.
    for (exports) |e| {
        try out.writer.writeAll(e.body);
    }

    return out.toOwnedSlice();
}
