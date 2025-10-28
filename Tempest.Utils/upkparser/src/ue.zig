const std = @import("std");
const mem = std.mem;

pub const ECompressionFlags = enum(u32) {
    COMPRESS_None = 0x00,
    COMPRESS_ZLIB = 0x01,
    COMPRESS_LZO = 0x02,
    COMPRESS_LZX = 0x04,
    COMPRESS_BiasMemory = 0x10,
    COMPRESS_BiasSpeed = 0x20,
    COMPRESS_ForcePPUDecompressZLib = 0x80,
    COMPRESS_NoStats = 0x100,
    COMPRESS_Obscured = 0x200,

    pub fn bit(self: ECompressionFlags) u32 {
        return @intFromEnum(self);
    }

    pub fn combine(comptime list: []const ECompressionFlags) u32 {
        var mask: u32 = 0;
        for (list) |flag| mask |= flag.bit();
        return mask;
    }

    pub fn has(mask: u32, flag: ECompressionFlags) bool {
        return (mask & flag.bit()) != 0;
    }

    pub fn hasAll(mask: u32, comptime list: []const ECompressionFlags) bool {
        const combined = ECompressionFlags.combine(list);
        return (mask & combined) == combined;
    }

    pub fn hasAny(mask: u32, comptime list: []const ECompressionFlags) bool {
        const combined = ECompressionFlags.combine(list);
        return (mask & combined) != 0;
    }

    pub fn add(mask: u32, comptime list: []const ECompressionFlags) u32 {
        return mask | ECompressionFlags.combine(list);
    }

    pub fn remove(mask: u32, comptime list: []const ECompressionFlags) u32 {
        return mask & ~ECompressionFlags.combine(list);
    }

    pub fn print(mask: u32) void {
        std.debug.print("ECompressionFlags set:\n", .{});
        inline for (@typeInfo(ECompressionFlags).@"enum".fields) |field| {
            if (mask & field.value != 0)
                std.debug.print("  - {s}\n", .{field.name});
        }
    }
};

pub const EPackageFlags = enum(u32) {
    PKG_AllowDownload = 0x00000001, // Allow downloading package.
    PKG_ClientOptional = 0x00000002, // Purely optional for clients.
    PKG_ServerSideOnly = 0x00000004, // Only needed on the server side.
    PKG_Cooked = 0x00000008, // Whether this package has been cooked for the target platform.
    PKG_Unsecure = 0x00000010, // Not trusted.
    PKG_SavedWithNewerVersion = 0x00000020, // Package was saved with newer version.
    PKG_Need = 0x00008000, // Client needs to download this package.
    PKG_Compiling = 0x00010000, // package is currently being compiled
    PKG_ContainsMap = 0x00020000, // Set if the package contains a ULevel/ UWorld object
    PKG_Trash = 0x00040000, // Set if the package was loaded from the trashcan
    PKG_DisallowLazyLoading = 0x00080000, // Set if the archive serializing this package cannot use lazy loading
    PKG_PlayInEditor = 0x00100000, // Set if the package was created for the purpose of PIE
    PKG_ContainsScript = 0x00200000, // Package is allowed to contain UClasses and unrealscript
    PKG_ContainsDebugInfo = 0x00400000, // Package contains debug info (for UDebugger)
    PKG_RequireImportsAlreadyLoaded = 0x00800000, // Package requires all its imports to already have been loaded
    PKG_StoreCompressed = 0x02000000, // Package is being stored compressed, requires archive support for compression
    PKG_StoreFullyCompressed = 0x04000000, // Package is serialized normally, and then fully compressed after (must be decompressed before LoadPackage is called)
    PKG_ContainsFaceFXData = 0x10000000, // Package contains FaceFX assets and/or animsets
    PKG_NoExportAllowed = 0x20000000, // Package was NOT created by a modder.  Internal data not for export
    PKG_StrippedSource = 0x40000000, // Source has been removed to compress the package size
    PKG_FilterEditorOnly = 0x80000000, // Package has editor-only data filtered

    pub fn bit(self: EPackageFlags) u32 {
        return @intFromEnum(self);
    }

    pub fn combine(comptime list: []const EPackageFlags) u32 {
        var mask: u32 = 0;
        for (list) |flag| mask |= flag.bit();
        return mask;
    }

    pub fn has(mask: u32, flag: EPackageFlags) bool {
        return (mask & flag.bit()) != 0;
    }

    pub fn hasAll(mask: u32, comptime list: []const EPackageFlags) bool {
        const combined = EPackageFlags.combine(list);
        return (mask & combined) == combined;
    }

    pub fn hasAny(mask: u32, comptime list: []const EPackageFlags) bool {
        const combined = EPackageFlags.combine(list);
        return (mask & combined) != 0;
    }

    pub fn add(mask: u32, comptime list: []const EPackageFlags) u32 {
        return mask | EPackageFlags.combine(list);
    }

    pub fn remove(mask: u32, comptime list: []const EPackageFlags) u32 {
        return mask & ~EPackageFlags.combine(list);
    }

    pub fn print(mask: u32) void {
        std.debug.print("EPackageFlags set:\n", .{});
        inline for (@typeInfo(EPackageFlags).@"enum".fields) |field| {
            if (mask & field.value != 0)
                std.debug.print("  - {s}\n", .{field.name});
        }
    }
};

pub const EObjectFlags = enum(u64) {
    RF_InSingularFunc = 0x0000000000000002, // In a singular function.
    RF_StateChanged = 0x0000000000000004, // Object did a state change.
    RF_DebugPostLoad = 0x0000000000000008, // For debugging PostLoad calls.
    RF_DebugSerialize = 0x0000000000000010, // For debugging Serialize calls.
    RF_DebugFinishDestroyed = 0x0000000000000020, // For debugging FinishDestroy calls.
    RF_EdSelected = 0x0000000000000040, // Object is selected in one of the editors browser windows.
    RF_ZombieComponent = 0x0000000000000080, // This component's template was deleted, so should not be used.
    RF_Protected = 0x0000000000000100, // Property is protected (may only be accessed from its owner class or subclasses)
    RF_ClassDefaultObject = 0x0000000000000200, // this object is its class's default object
    RF_ArchetypeObject = 0x0000000000000400, // this object is a template for another object - treat like a class default object
    RF_ForceTagExp = 0x0000000000000800, // Forces this object to be put into the export table when saving a package regardless of outer
    RF_TokenStreamAssembled = 0x0000000000001000, // Set if reference token stream has already been assembled
    RF_MisalignedObject = 0x0000000000002000, // Object's size no longer matches the size of its C++ class (only used during make, for native classes whose properties have changed)
    RF_RootSet = 0x0000000000004000, // Object will not be garbage collected, even if unreferenced.
    RF_BeginDestroyed = 0x0000000000008000, // BeginDestroy has been called on the object.
    RF_FinishDestroyed = 0x0000000000010000, // FinishDestroy has been called on the object.
    RF_DebugBeginDestroyed = 0x0000000000020000, // Whether object is rooted as being part of the root set (garbage collection)
    RF_MarkedByCooker = 0x0000000000040000, // Marked by content cooker.
    RF_LocalizedResource = 0x0000000000080000, // Whether resource object is localized.
    RF_InitializedProps = 0x0000000000100000, // whether InitProperties has been called on this object
    RF_PendingFieldPatches = 0x0000000000200000, //@script patcher: indicates that this struct will receive additional member properties from the script patcher
    RF_IsCrossLevelReferenced = 0x0000000000400000, // This object has been pointed to by a cross-level reference, and therefore requires additional cleanup upon deletion
    RF_Saved = 0x0000000080000000, // Object has been saved via SavePackage. Temporary.
    RF_Transactional = 0x0000000100000000, // Object is transactional.
    RF_Unreachable = 0x0000000200000000, // Object is not reachable on the object graph.
    RF_Public = 0x0000000400000000, // Object is visible outside its package.
    RF_TagImp = 0x0000000800000000, // Temporary import tag in load/save.
    RF_TagExp = 0x0000001000000000, // Temporary export tag in load/save.
    RF_Obsolete = 0x0000002000000000, // Object marked as obsolete and should be replaced.
    RF_TagGarbage = 0x0000004000000000, // Check during garbage collection.
    RF_DisregardForGC = 0x0000008000000000, // Object is being disregard for GC as its static and itself and all references are always loaded.
    RF_PerObjectLocalized = 0x0000010000000000, // Object is localized by instance name, not by class.
    RF_NeedLoad = 0x0000020000000000, // During load, indicates object needs loading.
    RF_AsyncLoading = 0x0000040000000000, // Object is being asynchronously loaded.
    RF_NeedPostLoadSubobjects = 0x0000080000000000, // During load, indicates that the object still needs to instance subobjects and fixup serialized component references
    RF_Suppress = 0x0000100000000000, // @warning: Mirrored in UnName.h. Suppressed log name.
    RF_InEndState = 0x0000200000000000, // Within an EndState call.
    RF_Transient = 0x0000400000000000, // Don't save object.
    RF_Cooked = 0x0000800000000000, // Whether the object has already been cooked
    RF_LoadForClient = 0x0001000000000000, // In-file load for client.
    RF_LoadForServer = 0x0002000000000000, // In-file load for client.
    RF_LoadForEdit = 0x0004000000000000, // In-file load for client.
    RF_Standalone = 0x0008000000000000, // Keep object around for editing even if unreferenced.
    RF_NotForClient = 0x0010000000000000, // Don't load this object for the game client.
    RF_NotForServer = 0x0020000000000000, // Don't load this object for the game server.
    RF_NotForEdit = 0x0040000000000000, // Don't load this object for the editor.
    RF_NeedPostLoad = 0x0100000000000000, // Object needs to be postloaded.
    RF_HasStack = 0x0200000000000000, // Has execution stack.
    RF_Native = 0x0400000000000000, // Native (UClass only).
    RF_Marked = 0x0800000000000000, // Marked (for debugging).
    RF_ErrorShutdown = 0x1000000000000000, // ShutdownAfterError called.
    RF_PendingKill = 0x2000000000000000, // Objects that are pending destruction (invalid for gameplay but valid objects)
    RF_MarkedByCookerTemp = 0x4000000000000000, // Temporarily marked by content cooker - should be cleared.
    RF_CookedStartupObject = 0x8000000000000000, // This object was cooked into a startup package.
    RF_AllFlags = 0xFFFFFFFFFFFFFFFF,

    pub fn bit(self: EObjectFlags) u64 {
        return @intFromEnum(self);
    }

    pub fn combine(comptime list: []const EObjectFlags) u64 {
        var mask: u64 = 0;
        for (list) |flag| mask |= flag.bit();
        return mask;
    }

    pub fn has(mask: u64, flag: EObjectFlags) bool {
        return (mask & flag.bit()) != 0;
    }

    pub fn hasAll(mask: u64, comptime list: []const EObjectFlags) bool {
        const combined = EObjectFlags.combine(list);
        return (mask & combined) == combined;
    }

    pub fn hasAny(mask: u64, comptime list: []const EObjectFlags) bool {
        const combined = EObjectFlags.combine(list);
        return (mask & combined) != 0;
    }

    pub fn add(mask: u64, comptime list: []const EObjectFlags) u64 {
        return mask | EObjectFlags.combine(list);
    }

    pub fn remove(mask: u64, comptime list: []const EObjectFlags) u64 {
        return mask & ~EObjectFlags.combine(list);
    }

    pub fn print(mask: u64) void {
        std.debug.print("EObjectFlags set:\n", .{});
        inline for (@typeInfo(EObjectFlags).@"enum".fields) |field| {
            if (mask & field.value != 0)
                std.debug.print("  - {s}\n", .{field.name});
        }
    }
};

pub const RF_ContextFlags = EObjectFlags.combine(&.{
    .RF_NotForClient, .RF_NotForServer, .RF_NotForEdit,
});

pub const RF_LoadContextFlags = EObjectFlags.combine(&.{
    .RF_LoadForClient, .RF_LoadForServer, .RF_LoadForEdit,
});

pub const RF_Load = EObjectFlags.combine(&.{
    .RF_ContextFlags, .RF_LoadContextFlags, .RF_Public, .RF_Standalone, .RF_Native, .RF_Obsolete, .RF_Protected, .RF_Transactional, .RF_HasStack, .RF_PerObjectLocalized, .RF_ClassDefaultObject, .RF_ArchetypeObject, .RF_LocalizedResource,
});

pub const RF_Keep = EObjectFlags.combine(&.{
    .RF_Native, .RF_Marked, .RF_PerObjectLocalized, .RF_MisalignedObject, .RF_DisregardForGC, .RF_RootSet, .RF_LocalizedResource,
});

pub const RF_ScriptMask = EObjectFlags.combine(&.{
    .RF_Transactional, .RF_Public, .RF_Transient, .RF_NotForClient, .RF_NotForServer, .RF_NotForEdit, .RF_Standalone,
});

pub const RF_UndoRedoMask = EObjectFlags.combine(&.{
    .RF_PendingKill,
});

pub const RF_PropagateToSubObjects = EObjectFlags.combine(&.{
    .RF_Public, .RF_ArchetypeObject, .RF_Transactional,
});

pub const FGuid = extern struct {
    a: u32,
    b: u32,
    c: u32,
    d: u32,

    pub fn formatNumber(self: FGuid, writer: *std.Io.Writer, number: std.fmt.Number) !void {
        _ = number;
        try writer.print("{X}-{X}-{X}-{X}", .{
            self.a,
            self.b,
            self.c,
            self.d,
        });
    }
};

pub const FNameEntry = extern struct {
    name: FName,
    flags: u64,

    pub fn read(reader: *std.Io.Reader, allocator: mem.Allocator) !FNameEntry {
        const name = try FName.read(reader, allocator);
        const flags = try reader.takeInt(u64, .little);
        return FNameEntry{ .name = name, .flags = flags };
    }

    pub fn write(self: FNameEntry, w: *std.io.Writer) !void {
        try self.name.write(w);
        try w.writeInt(u64, self.flags, .little);
    }
};

pub const FName = extern struct {
    len: u32,
    data: [*:0]u8,

    pub fn read(reader: *std.Io.Reader, allocator: mem.Allocator) !FName {
        const len: u32 = try reader.takeInt(u32, .little);
        const name = try reader.readAlloc(allocator, @intCast(len));
        errdefer allocator.free(name);

        return FName{ .len = len, .data = @ptrCast(name) };
    }

    pub fn readArray(reader: *std.Io.Reader, allocator: mem.Allocator) ![]FName {
        var len: u32 = try reader.takeInt(u32, .little);
        var names = try std.ArrayList(FName).initCapacity(allocator, len);
        errdefer names.deinit(allocator);

        while (len != 0) {
            const name = try FName.read(reader, allocator);
            try names.append(allocator, name);
            len -= 1;
        }

        return try names.toOwnedSlice(allocator);
    }

    pub fn write(self: FName, w: *std.io.Writer) !void {
        try w.writeInt(u32, self.len, .little);
        try w.writeAll(self.data[0..self.len]);
    }

    pub fn writeArray(self: []const FName, w: *std.io.Writer, len: u32) !void {
        try w.writeInt(u32, len, .little);
        for (self) |name| {
            try name.write(w);
        }
    }

    pub fn deinit(self: FName, allocator: mem.Allocator) void {
        allocator.free(self.data[0..self.len]);
    }

    pub fn toString(self: FName) []const u8 {
        return self.data[0..self.len];
    }
};

pub const FObjectImport = extern struct {
    class_package: u32 = 0,
    class_name: u32 = 0,
    outer_index: u32 = 0,
    unk1: u32 = 0,
    owner_ref: i32 = 0,
    object_name: u32 = 0,
    unk2: u32 = 0,

    pub fn read(r: *std.Io.Reader) !FObjectImport {
        const import: FObjectImport = .{
            .class_package = try r.takeInt(u32, .little),
            .class_name = try r.takeInt(u32, .little),
            .outer_index = try r.takeInt(u32, .little),
            .unk1 = try r.takeInt(u32, .little),
            .owner_ref = try r.takeInt(i32, .little),
            .object_name = try r.takeInt(u32, .little),
            .unk2 = try r.takeInt(u32, .little),
        };
        return import;
    }

    pub fn write(self: FObjectImport, w: *std.io.Writer) !void {
        try w.writeInt(u32, self.class_package, .little);
        try w.writeInt(u32, self.class_name, .little);
        try w.writeInt(u32, self.outer_index, .little);
        try w.writeInt(u32, self.unk1, .little);
        try w.writeInt(i32, self.owner_ref, .little);
        try w.writeInt(u32, self.object_name, .little);
        try w.writeInt(u32, self.unk2, .little);
    }

    pub fn debugPrint(self: FObjectImport) void {
        std.debug.print(
            \\FObjectImport:
            \\  class_package: {d}
            \\  class_name: {d}
            \\  outer_index: {d}
            \\  unk1: {d}
            \\  owner_ref: {d}
            \\  object_name: {d}
            \\  unk2: {d}
            \\
            \\
        , .{
            self.class_package,
            self.class_name,
            self.outer_index,
            self.unk1,
            self.owner_ref,
            self.object_name,
            self.unk2,
        });
    }
};

pub const FCompressedChunk = extern struct {
    UncompressedOffset: u32,
    UncompressedSize: u32,
    CompressedOffset: u32,
    CompressedSize: u32,

    pub fn read(r: *std.io.Reader, allocator: mem.Allocator) ![]FCompressedChunk {
        const count = try r.takeInt(u32, .little);
        const chunks = try allocator.alloc(FCompressedChunk, count);
        errdefer allocator.free(chunks);

        for (chunks) |*chunk| {
            chunk.* = try r.takeStruct(FCompressedChunk, .little);
        }

        return chunks;
    }

    pub fn write(self: FCompressedChunk, w: *std.io.Writer, count: u32) !void {
        try w.writeInt(u32, count, .little);
        for (self) |chunk| {
            try w.writeStruct(chunk, .little);
        }
    }

    pub fn debugPrint(self: FCompressedChunk) void {
        std.debug.print(
            \\FCompressedChunk:
            \\  UncompressedOffset: {d}
            \\  UncompressedSize:   {d}
            \\  CompressedOffset:   {d}
            \\  CompressedSize:     {d}
            \\
            \\
        , .{
            self.UncompressedOffset,
            self.UncompressedSize,
            self.CompressedOffset,
            self.CompressedSize,
        });
    }
};

pub const FTextureAllocation = struct {
    size_x: u32,
    size_y: u32,
    num_mips: u32,
    format: u32,
    tex_create_flags: u32,
    export_indices_count: u32,
    export_indices: [*]u32,

    pub fn read(r: *std.io.Reader, allocator: mem.Allocator) ![]FTextureAllocation {
        const count = try r.takeInt(u32, .little);
        const allocations = try allocator.alloc(FTextureAllocation, count);
        errdefer allocator.free(allocations);

        for (allocations) |*allocation| {
            allocation.* = .{
                .size_x = try r.takeInt(u32, .little),
                .size_y = try r.takeInt(u32, .little),
                .num_mips = try r.takeInt(u32, .little),
                .format = try r.takeInt(u32, .little),
                .tex_create_flags = try r.takeInt(u32, .little),
                .export_indices_count = try r.takeInt(u32, .little),
                .export_indices = &.{},
            };

            if (allocation.export_indices_count > 0) {
                var indices = try std.ArrayList(u32).initCapacity(allocator, allocation.export_indices_count);
                errdefer indices.deinit(allocator);

                var i: u32 = 0;
                while (i < allocation.export_indices_count) : (i += 1) {
                    try indices.append(allocator, try r.takeInt(u32, .little));
                }

                allocation.export_indices = (try indices.toOwnedSlice(allocator)).ptr;
            }
        }

        return allocations;
    }

    pub fn write(self: FTextureAllocation, w: *std.io.Writer) !void {
        try w.writeInt(u32, self.size_x, .little);
        try w.writeInt(u32, self.size_y, .little);
        try w.writeInt(u32, self.num_mips, .little);
        try w.writeInt(u32, self.format, .little);
        try w.writeInt(u32, self.tex_create_flags, .little);
        try w.writeInt(u32, self.export_indices_count, .little);

        if (self.export_indices_count > 0) {
            for (self.export_indices[0..self.export_indices_count]) |index| {
                try w.writeInt(u32, index, .little);
            }
        }
    }

    pub fn debugPrint(self: FTextureAllocation) void {
        std.debug.print(
            \\FTextureAllocation:
            \\  size_x: {d}
            \\  size_y: {d}
            \\  num_mips: {d}
            \\  format: {d}
            \\  tex_create_flags: {d}
            \\  export_indices_count: {d}
            \\
            \\
        , .{
            self.size_x,
            self.size_y,
            self.num_mips,
            self.format,
            self.tex_create_flags,
            self.export_indices_count,
        });
    }
};

pub const FObjectExport = extern struct {
    class_index: i32 = 0,
    super_index: i32 = 0,
    outer_index: i32 = 0,
    object_name: u32 = 0,
    archetype_index: u32 = 0,
    archetype: u32 = 0,
    object_flags: u64 = 0,
    serial_size: u32 = 0,
    serial_offset: u32 = 0,
    export_flags: u32 = 0,
    generation_net_object_count: u32 = 0,
    generation_net_objects: [*]u32 = &.{},
    package_guid: FGuid = .{ .a = 0, .b = 0, .c = 0, .d = 0 },
    package_flags: u32 = 0,

    pub fn read(r: *std.Io.Reader, allocator: mem.Allocator) !FObjectExport {
        var @"export": FObjectExport = .{
            .class_index = try r.takeInt(i32, .little),
            .super_index = try r.takeInt(i32, .little),
            .outer_index = try r.takeInt(i32, .little),
            .object_name = try r.takeInt(u32, .little),
            .archetype_index = try r.takeInt(u32, .little),
            .archetype = try r.takeInt(u32, .little),
            .object_flags = try r.takeInt(u64, .little),
            .serial_size = try r.takeInt(u32, .little),
            .serial_offset = try r.takeInt(u32, .little),
            .export_flags = try r.takeInt(u32, .little),
            .generation_net_object_count = try r.takeInt(u32, .little),
            .package_guid = try r.takeStruct(FGuid, .little),
            .package_flags = try r.takeInt(u32, .little),
        };

        if (@"export".generation_net_object_count > 0) {
            var net_objects = try std.ArrayList(u32).initCapacity(allocator, @"export".generation_net_object_count);
            errdefer net_objects.deinit(allocator);

            var i: u32 = 0;
            while (i < @"export".generation_net_object_count) : (i += 1) {
                try net_objects.append(allocator, try r.takeInt(u32, .little));
            }

            @"export".generation_net_objects = (try net_objects.toOwnedSlice(allocator)).ptr;
        }

        return @"export";
    }

    pub fn write(self: FObjectExport, w: *std.io.Writer) !void {
        try w.writeInt(i32, self.class_index, .little);
        try w.writeInt(i32, self.super_index, .little);
        try w.writeInt(i32, self.outer_index, .little);
        try w.writeInt(u32, self.object_name, .little);
        try w.writeInt(u32, self.archetype_index, .little);
        try w.writeInt(u32, self.archetype, .little);
        try w.writeInt(u64, self.object_flags, .little);
        try w.writeInt(u32, self.serial_size, .little);
        try w.writeInt(u32, self.serial_offset, .little);
        try w.writeInt(u32, self.export_flags, .little);
        try w.writeInt(u32, self.generation_net_object_count, .little);
        try w.writeStruct(self.package_guid, .little);
        if (self.generation_net_object_count > 0) {
            for (self.generation_net_objects[0..self.generation_net_object_count]) |net_object| {
                try w.writeInt(u32, net_object, .little);
            }
        }
        try w.writeInt(u32, self.package_flags, .little);
    }

    pub fn debugPrint(self: FObjectExport) void {
        std.debug.print(
            \\FObjectExport:
            \\  class_index: {d}
            \\  super_index: {d}
            \\  outer_index: {d}
            \\  object_name: {d}
            \\  archetype_index: {d}
            \\  archetype: {d}
            \\  object_flags: {d}
            \\  serial_size: {d}
            \\  serial_offset: {d}
            \\  export_flags: {d}
            \\  generation_net_object_count: {d}
            \\  package_guid: {d}
            \\  package_flags: {d}
            \\
            \\
        , .{
            self.class_index,
            self.super_index,
            self.outer_index,
            self.object_name,
            self.archetype_index,
            self.archetype,
            self.object_flags,
            self.serial_size,
            self.serial_offset,
            self.export_flags,
            self.generation_net_object_count,
            self.package_guid,
            self.package_flags,
        });
    }
};

pub const FGenerationInfo = extern struct {
    export_count: i32,
    name_count: i32,
    net_object_count: i32,

    pub fn read(reader: *std.Io.Reader, allocator: mem.Allocator) ![]FGenerationInfo {
        const count = try reader.takeInt(u32, .little);
        const generations = try allocator.alloc(FGenerationInfo, count);

        for (generations) |*generation| {
            generation.* = FGenerationInfo{
                .export_count = try reader.takeInt(i32, .little),
                .name_count = try reader.takeInt(i32, .little),
                .net_object_count = try reader.takeInt(i32, .little),
            };
        }
        return generations;
    }

    pub fn write(self: FGenerationInfo, w: *std.io.Writer) !void {
        try w.writeInt(i32, self.export_count, .little);
        try w.writeInt(i32, self.name_count, .little);
        try w.writeInt(i32, self.net_object_count, .little);
    }

    pub fn debugPrint(self: FGenerationInfo) void {
        std.debug.print(
            \\FGenerationInfo:
            \\  export_count: {d}
            \\  name_count: {d}
            \\  net_object_count: {d}
            \\
            \\
        , .{
            self.export_count,
            self.name_count,
            self.net_object_count,
        });
    }
};
pub const FPackageFileSummary = struct {
    tag: u32,
    file_version: u16,
    licensee_version: u16,
    total_header_size: u32,
    folder_name: FName,
    package_flags: u32,
    name_count: u32,
    name_offset: u32,
    export_count: u32,
    export_offset: u32,
    import_count: u32,
    import_offset: u32,
    depends_offset: u32,
    import_export_guids_offset: u32,
    import_guids_count: u32,
    export_guids_count: u32,
    thumbnail_table_offset: u32,
    guid: FGuid,
    generations: []FGenerationInfo,
    engine_version: u32,
    cooker_version_upper: u16,
    cooker_version_lower: u16,
    compression_flags: u32,
    compressed_chunks: []FCompressedChunk,
    package_source: u32,
    additional_packages: []FName,
    texture_allocations: []FTextureAllocation,

    pub fn read(r: *std.Io.Reader, allocator: mem.Allocator) !FPackageFileSummary {
        const summary: FPackageFileSummary = .{
            .tag = try r.takeInt(u32, .little),
            .file_version = try r.takeInt(u16, .little),
            .licensee_version = try r.takeInt(u16, .little),
            .total_header_size = try r.takeInt(u32, .little),
            .folder_name = try FName.read(r, allocator),
            .package_flags = try r.takeInt(u32, .little) & ~@intFromEnum(EPackageFlags.PKG_FilterEditorOnly),
            .name_count = try r.takeInt(u32, .little),
            .name_offset = try r.takeInt(u32, .little),
            .export_count = try r.takeInt(u32, .little),
            .export_offset = try r.takeInt(u32, .little),
            .import_count = try r.takeInt(u32, .little),
            .import_offset = try r.takeInt(u32, .little),
            .depends_offset = try r.takeInt(u32, .little),
            .import_export_guids_offset = try r.takeInt(u32, .little),
            .import_guids_count = try r.takeInt(u32, .little),
            .export_guids_count = try r.takeInt(u32, .little),
            .thumbnail_table_offset = try r.takeInt(u32, .little),
            .guid = try r.takeStruct(FGuid, .little),
            .generations = try FGenerationInfo.read(r, allocator),
            .engine_version = try r.takeInt(u32, .little),
            .cooker_version_upper = try r.takeInt(u16, .little),
            .cooker_version_lower = try r.takeInt(u16, .little),
            .compression_flags = try r.takeInt(u32, .little),
            .compressed_chunks = try FCompressedChunk.read(r, allocator),
            .package_source = try r.takeInt(u32, .little),
            .additional_packages = try FName.readArray(r, allocator),
            .texture_allocations = try FTextureAllocation.read(r, allocator),
        };
        return summary;
    }

    pub fn write(self: FPackageFileSummary, w: *std.io.Writer) !void {
        try w.writeInt(u32, self.tag, .little);
        try w.writeInt(u16, self.file_version, .little);
        try w.writeInt(u16, self.licensee_version, .little);
        try w.writeInt(u32, self.total_header_size, .little);
        try self.folder_name.write(w);
        try w.writeInt(u32, self.package_flags | @intFromEnum(EPackageFlags.PKG_FilterEditorOnly), .little);
        try w.writeInt(u32, self.name_count, .little);
        try w.writeInt(u32, self.name_offset, .little);
        try w.writeInt(u32, self.export_count, .little);
        try w.writeInt(u32, self.export_offset, .little);
        try w.writeInt(u32, self.import_count, .little);
        try w.writeInt(u32, self.import_offset, .little);
        try w.writeInt(u32, self.depends_offset, .little);
        try w.writeInt(u32, self.import_export_guids_offset, .little);
        try w.writeInt(u32, self.import_guids_count, .little);
        try w.writeInt(u32, self.export_guids_count, .little);
        try w.writeInt(u32, self.thumbnail_table_offset, .little);
        try w.writeStruct(self.guid, .little);
        try w.writeInt(u32, @intCast(self.generations.len), .little);
        for (self.generations) |generation| {
            try generation.write(w);
        }
        try w.writeInt(u32, self.engine_version, .little);
        try w.writeInt(u16, self.cooker_version_upper, .little);
        try w.writeInt(u16, self.cooker_version_lower, .little);
        try w.writeInt(u32, self.compression_flags, .little);
        for (self.compressed_chunks) |chunk| {
            try chunk.write(w, self.compressed_chunks.len);
        }
        try w.writeInt(u32, self.package_source, .little);
        try w.writeInt(u32, @intCast(self.additional_packages.len), .little);
        for (self.additional_packages) |pkg| {
            try pkg.write(w);
        }
        try w.writeInt(u32, @intCast(self.texture_allocations.len), .little);
        for (self.texture_allocations) |allocation| {
            try allocation.write(w);
        }
    }

    pub fn debugPrint(self: FPackageFileSummary) void {
        std.debug.print(
            \\FPackageFileSummary:
            \\  tag:                        0x{X}
            \\  file_version:               {d}/{d}
            \\  total_header_size:          {d}
            \\  folder_name:                {s}
            \\  package_flags:              0x{X}
            \\  name_count:                 {d}
            \\  name_offset:                {d}
            \\  export_count:               {d}
            \\  export_offset:              {d}
            \\  import_count:               {d}
            \\  import_offset:              {d}
            \\  depends_offset:             {d}
            \\  import_export_guids_offset: {d}
            \\  import_guids_count:         {d}
            \\  export_guids_count:         {d}
            \\  thumbnail_table_offset:     {d}
            \\  guid:                       {d}
            \\  engine_version:             {d}
            \\  cooker_version:             {d}/{d}
            \\  compression_flags:          {d}
            \\  compressed_chunks:          {d}
            \\  package_source:             0x{X}
            \\  additional_packages_count:  {d}
            \\  texture_allocations:        {d}
            \\
            \\
        , .{
            self.tag,
            self.file_version,
            self.licensee_version,
            self.total_header_size,
            self.folder_name.toString(),
            self.package_flags,
            self.name_count,
            self.name_offset,
            self.export_count,
            self.export_offset,
            self.import_count,
            self.import_offset,
            self.depends_offset,
            self.import_export_guids_offset,
            self.import_guids_count,
            self.export_guids_count,
            self.thumbnail_table_offset,
            self.guid,
            self.engine_version,
            self.cooker_version_upper,
            self.cooker_version_lower,
            self.compression_flags,
            self.compressed_chunks.len,
            self.package_source,
            self.additional_packages.len,
            self.texture_allocations.len,
        });

        std.debug.print("Flags:\n", .{});
        EPackageFlags.print(self.package_flags);

        if (self.additional_packages.len > 0) {
            std.debug.print("\nAdditional packages:\n", .{});
            for (self.additional_packages, 0..) |pkg, i| {
                std.debug.print("  {d}: {s}\n", .{ i, pkg.toString() });
            }
        }

        std.debug.print("\nGenerations:\n", .{});
        for (self.generations) |generation| {
            generation.debugPrint();
        }

        if (self.texture_allocations.len > 0) {
            std.debug.print("\nTexture allocations:\n", .{});
            for (self.texture_allocations) |allocation| {
                allocation.debugPrint();
            }
        }

        if (self.compressed_chunks.len > 0) {
            std.debug.print("\nCompressed chunks:\n", .{});
            for (self.compressed_chunks) |chunk| {
                chunk.debugPrint();
            }
        }
    }
};
