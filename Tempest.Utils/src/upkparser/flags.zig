const std = @import("std");

pub const ECompressionFlags = packed struct(u32) {
    type: enum(u3) { none, zlib, lzo, lzx } = .none,
    _pad1: u1 = 0,
    bias_memory: bool = false,
    bias_speed: bool = false,
    _pad2: u1 = 0,
    force_ppu_decompress_zlib: bool = false,
    no_stats: bool = false,
    obscured: bool = false,
    _pad3: u22 = 0,

    pub fn format(self: ECompressionFlags, writer: *std.Io.Writer) !void {
        try writer.print("{s}", .{@tagName(self.type)});

        inline for (@typeInfo(ECompressionFlags).@"struct".fields) |field| {
            if (comptime std.mem.eql(u8, field.name, "type")) continue;
            if (comptime std.mem.startsWith(u8, field.name, "_pad")) continue;

            if (field.type == bool) {
                if (@field(self, field.name)) {
                    try writer.print(", {s}", .{field.name});
                }
            }
        }
    }
};

pub const EPackageFlags = packed struct(u32) {
    allow_download: bool = false,
    client_optional: bool = false,
    server_side_only: bool = false,
    cooked: bool = false,
    unsecure: bool = false,
    saved_with_newer_version: bool = false,
    _pad1: u9 = 0,
    need: bool = false,
    compiling: bool = false,
    contains_map: bool = false,
    trash: bool = false,
    disallow_lazy_loading: bool = false,
    play_in_editor: bool = false,
    contains_script: bool = false,
    contains_debug_info: bool = false,
    require_imports_already_loaded: bool = false,
    _pad2: u1 = 0,
    store_compressed: bool = false,
    store_fully_compressed: bool = false,
    _pad3: u1 = 0,
    contains_face_fx_data: bool = false,
    no_export_allowed: bool = false,
    stripped_source: bool = false,
    filter_editor_only: bool = false,

    pub fn format(self: EPackageFlags, writer: *std.Io.Writer) !void {
        inline for (@typeInfo(EPackageFlags).@"struct".fields, 0..) |field, i| {
            if (comptime std.mem.startsWith(u8, field.name, "_pad")) continue;

            if (field.type == bool) {
                if (@field(self, field.name)) {
                    try writer.print("{s}{s}", .{
                        if (i == 0) "" else ", ",
                        field.name,
                    });
                }
            }
        }
    }
};

pub const EObjectFlags = packed struct(u64) {
    _pad1: u1,
    in_singular_func: bool,
    state_changed: bool,
    debug_post_load: bool,
    debug_serialize: bool,
    debug_finish_destroyed: bool,
    ed_selected: bool,
    zombie_component: bool,
    protected: bool,
    class_default_object: bool,
    archetype_object: bool,
    force_tag_exp: bool,
    token_stream_assembled: bool,
    misaligned_object: bool,
    root_set: bool,
    begin_destroyed: bool,
    finish_destroyed: bool,
    debug_begin_destroyed: bool,
    marked_by_cooker: bool,
    localized_resource: bool,
    initialized_props: bool,
    pending_field_patches: bool,
    is_cross_level_referenced: bool,
    _pad2: u8,
    saved: bool,
    transactional: bool,
    @"unreachable": bool,
    public: bool,
    tag_imp: bool,
    tag_exp: bool,
    obsolete: bool,
    tag_garbage: bool,
    disregard_for_gc: bool,
    per_object_localized: bool,
    need_load: bool,
    async_loading: bool,
    need_post_load_subobjects: bool,
    suppress: bool,
    in_end_state: bool,
    transient: bool,
    cooked: bool,
    load_for_client: bool,
    load_for_server: bool,
    load_for_edit: bool,
    standalone: bool,
    not_for_client: bool,
    not_for_server: bool,
    not_for_edit: bool,
    _pad3: u4,
    need_post_load: bool,
    has_stack: bool,
    native: bool,
    marked: bool,
    error_shutdown: bool,
    pending_kill: bool,
    marked_by_cooker_temp: bool,
    cooked_startup_object: bool,

    pub fn format(self: EPackageFlags, writer: *std.Io.Writer) !void {
        inline for (@typeInfo(EPackageFlags).@"struct".fields, 0..) |field, i| {
            if (comptime std.mem.startsWith(u8, field.name, "_pad")) continue;

            if (field.type == bool) {
                if (@field(self, field.name)) {
                    try writer.print("{s}{s}", .{
                        if (i == 0) "" else ", ",
                        field.name,
                    });
                }
            }
        }
    }
};

// pub const RF_ContextFlags = EObjectFlags.combine(&.{
//     .RF_NotForClient, .RF_NotForServer, .RF_NotForEdit,
// });
//
// pub const RF_LoadContextFlags = EObjectFlags.combine(&.{
//     .RF_LoadForClient, .RF_LoadForServer, .RF_LoadForEdit,
// });
//
// pub const RF_Load = EObjectFlags.combine(&.{
//     .RF_ContextFlags, .RF_LoadContextFlags, .RF_Public, .RF_Standalone, .RF_Native, .RF_Obsolete, .RF_Protected, .RF_Transactional, .RF_HasStack, .RF_PerObjectLocalized, .RF_ClassDefaultObject, .RF_ArchetypeObject, .RF_LocalizedResource,
// });
//
// pub const RF_Keep = EObjectFlags.combine(&.{
//     .RF_Native, .RF_Marked, .RF_PerObjectLocalized, .RF_MisalignedObject, .RF_DisregardForGC, .RF_RootSet, .RF_LocalizedResource,
// });
//
// pub const RF_ScriptMask = EObjectFlags.combine(&.{
//     .RF_Transactional, .RF_Public, .RF_Transient, .RF_NotForClient, .RF_NotForServer, .RF_NotForEdit, .RF_Standalone,
// });
//
// pub const RF_UndoRedoMask = EObjectFlags.combine(&.{
//     .RF_PendingKill,
// });
//
// pub const RF_PropagateToSubObjects = EObjectFlags.combine(&.{
//     .RF_Public, .RF_ArchetypeObject, .RF_Transactional,
// });
