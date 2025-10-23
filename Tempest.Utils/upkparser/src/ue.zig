const std = @import("std");
const mem = std.mem;

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

    pub inline fn bit(self: EPackageFlags) u32 {
        return @intFromEnum(self);
    }

    pub inline fn combine(comptime list: []const EPackageFlags) u32 {
        var mask: u32 = 0;
        for (list) |flag| mask |= flag.bit();
        return mask;
    }

    pub inline fn has(mask: u32, flag: EPackageFlags) bool {
        return (mask & flag.bit()) != 0;
    }

    pub inline fn hasAll(mask: u32, comptime list: []const EPackageFlags) bool {
        const combined = EPackageFlags.combine(list);
        return (mask & combined) == combined;
    }

    pub inline fn hasAny(mask: u32, comptime list: []const EPackageFlags) bool {
        const combined = EPackageFlags.combine(list);
        return (mask & combined) != 0;
    }

    pub inline fn add(mask: u32, comptime list: []const EPackageFlags) u32 {
        return mask | EPackageFlags.combine(list);
    }

    pub inline fn remove(mask: u32, comptime list: []const EPackageFlags) u32 {
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

    pub inline fn bit(self: EObjectFlags) u64 {
        return @intFromEnum(self);
    }

    pub inline fn combine(comptime list: []const EObjectFlags) u64 {
        var mask: u64 = 0;
        for (list) |flag| mask |= flag.bit();
        return mask;
    }

    pub inline fn has(mask: u64, flag: EObjectFlags) bool {
        return (mask & flag.bit()) != 0;
    }

    pub inline fn hasAll(mask: u64, comptime list: []const EObjectFlags) bool {
        const combined = EObjectFlags.combine(list);
        return (mask & combined) == combined;
    }

    pub inline fn hasAny(mask: u64, comptime list: []const EObjectFlags) bool {
        const combined = EObjectFlags.combine(list);
        return (mask & combined) != 0;
    }

    pub inline fn add(mask: u64, comptime list: []const EObjectFlags) u64 {
        return mask | EObjectFlags.combine(list);
    }

    pub inline fn remove(mask: u64, comptime list: []const EObjectFlags) u64 {
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
};

pub const FName = extern struct {
    len: u32,
    name: [*:0]u8,

    pub fn read(reader: *std.Io.Reader, allocator: mem.Allocator) !FName {
        const len: u32 = try reader.takeInt(u32, .little);
        const name = try reader.readAlloc(allocator, @intCast(len));
        errdefer allocator.free(name);

        return FName{ .len = len, .name = @ptrCast(name) };
    }

    pub fn deinit(self: FName, allocator: mem.Allocator) void {
        allocator.free(self.name[0..self.len]);
    }

    pub fn toString(self: FName) []const u8 {
        return self.name[0..self.len];
    }
};

// TODO: find the actual structs in the engine
// this is currently using the XCOM:EU 2012 reversed names
pub const FObjectImport = extern struct {
    PackageIdIndex: u32 = 0,
    Field1: u32 = 0,
    ObjTypeIndex: u32 = 0,
    Field2: u32 = 0,
    OwnerRef: i32 = 0,
    NameTableIndex: u32 = 0,
    Field3: u32 = 0,

    pub fn read(r: *std.Io.Reader) !FObjectImport {
        const import: FObjectImport = .{
            .PackageIdIndex = try r.takeInt(u32, .little),
            .Field1 = try r.takeInt(u32, .little),
            .ObjTypeIndex = try r.takeInt(u32, .little),
            .Field2 = try r.takeInt(u32, .little),
            .OwnerRef = try r.takeInt(i32, .little),
            .NameTableIndex = try r.takeInt(u32, .little),
            .Field3 = try r.takeInt(u32, .little),
        };
        return import;
    }
};

pub const FObjectExport = extern struct {
    ClassIndex: i32 = 0,
    SuperIndex: i32 = 0,
    ArchetypeIndex: i32 = 0,
    NameTableIndex: u32 = 0,
    NameCount: u32 = 0,
    Field6: u32 = 0,
    ObjectFlags: u64 = 0,
    SerialSize: u32 = 0,
    SerialOffset: u32 = 0,
    Field11: u32 = 0,
    NumAdditionalFields: u32 = 0,
    Field13: u32 = 0,
    Field14: u32 = 0,
    Field15: u32 = 0,
    Field16: u32 = 0,
    Field17: u32 = 0,

    pub fn read(r: *std.Io.Reader) !FObjectExport {
        const @"export": FObjectExport = .{
            .ClassIndex = try r.takeInt(i32, .little),
            .SuperIndex = try r.takeInt(i32, .little),
            .ArchetypeIndex = try r.takeInt(i32, .little),
            .NameTableIndex = try r.takeInt(u32, .little),
            .NameCount = try r.takeInt(u32, .little),
            .Field6 = try r.takeInt(u32, .little),
            .ObjectFlags = try r.takeInt(u64, .little),
            .SerialSize = try r.takeInt(u32, .little),
            .SerialOffset = try r.takeInt(u32, .little),
            .Field11 = try r.takeInt(u32, .little),
            .NumAdditionalFields = try r.takeInt(u32, .little),
            .Field13 = try r.takeInt(u32, .little),
            .Field14 = try r.takeInt(u32, .little),
            .Field15 = try r.takeInt(u32, .little),
            .Field16 = try r.takeInt(u32, .little),
            .Field17 = try r.takeInt(u32, .little),
        };

        // TODO: figure out what these are
        r.toss(4 * @"export".NumAdditionalFields);

        return @"export";
    }

    pub fn write(self: *const FObjectExport, w: *std.Io.Writer) !void {
        try w.writeInt(i32, self.ClassIndex, .little);
        try w.writeInt(i32, self.SuperIndex, .little);
        try w.writeInt(i32, self.ArchetypeIndex, .little);
        try w.writeInt(u32, self.NameTableIndex, .little);
        try w.writeInt(u32, self.NameCount, .little);
        try w.writeInt(u32, self.Field6, .little);
        try w.writeInt(u64, self.ObjectFlags, .little);
        try w.writeInt(u32, self.SerialSize, .little);
        try w.writeInt(u32, self.SerialOffset, .little);
        try w.writeInt(u32, self.Field11, .little);
        try w.writeInt(u32, self.NumAdditionalFields, .little);
        try w.writeInt(u32, self.Field13, .little);
        try w.writeInt(u32, self.Field14, .little);
        try w.writeInt(u32, self.Field15, .little);
        try w.writeInt(u32, self.Field16, .little);
        try w.writeInt(u32, self.Field17, .little);

        // TODO: write the correct fields instead of zeroing them
        var i: u32 = 0;
        while (i < self.NumAdditionalFields) : (i += 1) {
            try w.writeInt(u32, 0, .little);
        }
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
    unk1: u32,
    unk2: u32,
    unk3: u32,
    unk4: u32,
    guid: FGuid,
    generations: []FGenerationInfo,
    engine_version: u32,
    cooker_version_upper: u16,
    cooker_version_lower: u16,
    compression_flags: u32,

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
            .unk1 = try r.takeInt(u32, .little),
            .unk2 = try r.takeInt(u32, .little),
            .unk3 = try r.takeInt(u32, .little),
            .unk4 = try r.takeInt(u32, .little),
            .guid = try r.takeStruct(FGuid, .little),
            .generations = try FGenerationInfo.read(r, allocator),
            .engine_version = try r.takeInt(u32, .little),
            .cooker_version_upper = try r.takeInt(u16, .little),
            .cooker_version_lower = try r.takeInt(u16, .little),
            .compression_flags = try r.takeInt(u32, .little),
        };
        return summary;
    }

    pub fn print(self: FPackageFileSummary) void {
        std.log.info("Package File Summary", .{});
        std.log.info(
            \\
            \\  tag:               0x{X}
            \\  file_version:      {d}/{d}
            \\  total_header_size: {d}
            \\  folder_name:       {s}
            \\  package_flags:     0x{X}
            \\  name_count:        {d}
            \\  name_offset:       {d}
            \\  export_count:      {d}
            \\  export_offset:     {d}
            \\  import_count:      {d}
            \\  import_offset:     {d}
            \\  depends_offset:    {d}
            \\  unk1:              {d}
            \\  unk2:              {d}
            \\  unk3:              {d}
            \\  unk4:              {d}
            \\  guid:              {d}
            \\  engine_version:    {d}
            \\  cooker_version:    {d}/{d}
            \\  compression_flags: {d}
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
            self.unk1,
            self.unk2,
            self.unk3,
            self.unk4,
            self.guid,
            self.engine_version,
            self.cooker_version_upper,
            self.cooker_version_lower,
            self.compression_flags,
        });
    }
};
