// Bytecode opcodes and flag constants for the UE3 script VM.
// Values mirror EExprToken / ECastToken (Core/Inc/UnStack.h), EPropertyType
// (Core/Inc/UnScript.h), and the E*Flags enums (Core/Inc/UnObjBas.h).

pub const Expr = enum(u8) {
    local_variable = 0x00,
    instance_variable = 0x01,
    default_variable = 0x02,
    state_variable = 0x03,
    return_ = 0x04,
    switch_ = 0x05,
    jump = 0x06,
    jump_if_not = 0x07,
    stop = 0x08,
    assert = 0x09,
    case_ = 0x0a,
    nothing = 0x0b,
    label_table = 0x0c,
    goto_label = 0x0d,
    eat_return_value = 0x0e,
    let = 0x0f,
    dyn_array_element = 0x10,
    new_ = 0x11,
    class_context = 0x12,
    meta_cast = 0x13,
    let_bool = 0x14,
    end_parm_value = 0x15,
    end_function_parms = 0x16,
    self = 0x17,
    skip = 0x18,
    context = 0x19,
    array_element = 0x1a,
    virtual_function = 0x1b,
    final_function = 0x1c,
    int_const = 0x1d,
    float_const = 0x1e,
    string_const = 0x1f,
    object_const = 0x20,
    name_const = 0x21,
    rotation_const = 0x22,
    vector_const = 0x23,
    byte_const = 0x24,
    int_zero = 0x25,
    int_one = 0x26,
    true_ = 0x27,
    false_ = 0x28,
    native_parm = 0x29,
    no_object = 0x2a,
    int_const_byte = 0x2c,
    bool_variable = 0x2d,
    dynamic_cast = 0x2e,
    iterator = 0x2f,
    iterator_pop = 0x30,
    iterator_next = 0x31,
    struct_cmp_eq = 0x32,
    struct_cmp_ne = 0x33,
    unicode_string_const = 0x34,
    struct_member = 0x35,
    dyn_array_length = 0x36,
    global_function = 0x37,
    primitive_cast = 0x38,
    dyn_array_insert = 0x39,
    return_nothing = 0x3a,
    equal_equal_del_del = 0x3b,
    not_equal_del_del = 0x3c,
    equal_equal_del_func = 0x3d,
    not_equal_del_func = 0x3e,
    empty_delegate = 0x3f,
    dyn_array_remove = 0x40,
    debug_info = 0x41,
    delegate_function = 0x42,
    delegate_property = 0x43,
    let_delegate = 0x44,
    conditional = 0x45,
    dyn_array_find = 0x46,
    dyn_array_find_struct = 0x47,
    local_out_variable = 0x48,
    default_parm_value = 0x49,
    empty_parm_value = 0x4a,
    instance_delegate = 0x4b,
    interface_context = 0x51,
    interface_cast = 0x52,
    end_of_script = 0x53,
    dyn_array_add = 0x54,
    dyn_array_add_item = 0x55,
    dyn_array_remove_item = 0x56,
    dyn_array_insert_item = 0x57,
    dyn_array_iterator = 0x58,
    dyn_array_sort = 0x59,
    jump_if_filter_editor_only = 0x5a,

    /// First native function opcode.
    extended_native = 0x60,
    first_native = 0x70,
};

/// Cast tokens (ECastToken) used as the payload of EX_PrimitiveCast.
pub const Cast = enum(u8) {
    interface_to_object = 0x36,
    interface_to_string = 0x37,
    interface_to_bool = 0x38,
    rotator_to_vector = 0x39,
    byte_to_int = 0x3a,
    byte_to_bool = 0x3b,
    byte_to_float = 0x3c,
    int_to_byte = 0x3d,
    int_to_bool = 0x3e,
    int_to_float = 0x3f,
    bool_to_byte = 0x40,
    bool_to_int = 0x41,
    bool_to_float = 0x42,
    float_to_byte = 0x43,
    float_to_int = 0x44,
    float_to_bool = 0x45,
    object_to_interface = 0x46,
    object_to_bool = 0x47,
    name_to_bool = 0x48,
    string_to_byte = 0x49,
    string_to_int = 0x4a,
    string_to_bool = 0x4b,
    string_to_float = 0x4c,
    string_to_vector = 0x4d,
    string_to_rotator = 0x4e,
    vector_to_bool = 0x4f,
    vector_to_rotator = 0x50,
    rotator_to_bool = 0x51,
    byte_to_string = 0x52,
    int_to_string = 0x53,
    bool_to_string = 0x54,
    float_to_string = 0x55,
    object_to_string = 0x56,
    name_to_string = 0x57,
    vector_to_string = 0x58,
    rotator_to_string = 0x59,
    delegate_to_string = 0x5a,
    string_to_name = 0x60,
};

/// Property base types (EPropertyType).
pub const PropType = enum(u8) {
    none = 0,
    byte = 1,
    int_ = 2,
    bool_ = 3,
    float_ = 4,
    object_reference = 5,
    name = 6,
    delegate = 7,
    interface = 8,
    range = 9,
    struct_ = 10,
    vector = 11,
    rotation = 12,
    string = 13,
    map = 14,
};

/// CPF_* property flags (u64).
pub const cp = struct {
    pub const edit: u64 = 0x0000000000000001;
    pub const const_: u64 = 0x0000000000000002;
    pub const input: u64 = 0x0000000000000004;
    pub const export_object: u64 = 0x0000000000000008;
    pub const optional_parm: u64 = 0x0000000000000010;
    pub const net: u64 = 0x0000000000000020;
    pub const edit_fixed_size: u64 = 0x0000000000000040;
    pub const parm: u64 = 0x0000000000000080;
    pub const out_parm: u64 = 0x0000000000000100;
    pub const skip_parm: u64 = 0x0000000000000200;
    pub const return_parm: u64 = 0x0000000000000400;
    pub const coerce_parm: u64 = 0x0000000000000800;
    pub const native: u64 = 0x0000000000001000;
    pub const transient: u64 = 0x0000000000002000;
    pub const config: u64 = 0x0000000000004000;
    pub const localized: u64 = 0x0000000000008000;
    pub const edit_const: u64 = 0x0000000000020000;
    pub const global_config: u64 = 0x0000000000040000;
    pub const component: u64 = 0x0000000000080000;
    pub const always_init: u64 = 0x0000000000100000;
    pub const duplicate_transient: u64 = 0x0000000000200000;
    pub const need_ctor_link: u64 = 0x0000000000400000;
    pub const no_export: u64 = 0x0000000000800000;
    pub const no_clear: u64 = 0x0000000002000000;
    pub const edit_inline: u64 = 0x0000000004000000;
    pub const edit_inline_use: u64 = 0x0000000010000000;
    pub const deprecated: u64 = 0x0000000020000000;
    pub const data_binding: u64 = 0x0000000040000000;
    pub const serialize_text: u64 = 0x0000000080000000;
    pub const rep_notify: u64 = 0x0000000100000000;
    pub const interp: u64 = 0x0000000200000000;
    pub const non_transactional: u64 = 0x0000000400000000;
    pub const editor_only: u64 = 0x0000000800000000;
    pub const not_for_console: u64 = 0x0000001000000000;
    pub const rep_retry: u64 = 0x0000002000000000;
    pub const private_write: u64 = 0x0000004000000000;
    pub const protected_write: u64 = 0x0000008000000000;
    pub const archetype_property: u64 = 0x0000010000000000;
    pub const edit_hide: u64 = 0x0000020000000000;
    pub const edit_text_box: u64 = 0x0000040000000000;
    pub const cross_level_passive: u64 = 0x0000100000000000;
    pub const cross_level_active: u64 = 0x0000200000000000;

    pub const parm_flags: u64 = optional_parm | parm | out_parm | skip_parm | return_parm | coerce_parm;
    pub const propagate_from_struct: u64 = const_ | native | transient;
    pub const cross_level: u64 = cross_level_passive | cross_level_active;
};

/// FUNC_* function flags (u32).
pub const func = struct {
    pub const final_: u32 = 0x00000001;
    pub const defined: u32 = 0x00000002;
    pub const iterator: u32 = 0x00000004;
    pub const latent: u32 = 0x00000008;
    pub const pre_operator: u32 = 0x00000010;
    pub const singular: u32 = 0x00000020;
    pub const net: u32 = 0x00000040;
    pub const net_reliable: u32 = 0x00000080;
    pub const simulated: u32 = 0x00000100;
    pub const exec: u32 = 0x00000200;
    pub const native: u32 = 0x00000400;
    pub const event: u32 = 0x00000800;
    pub const operator: u32 = 0x00001000;
    pub const static_: u32 = 0x00002000;
    pub const has_optional_parms: u32 = 0x00004000;
    pub const const_: u32 = 0x00008000;
    pub const public_: u32 = 0x00020000;
    pub const private_: u32 = 0x00040000;
    pub const protected_: u32 = 0x00080000;
    pub const delegate: u32 = 0x00100000;
    pub const net_server: u32 = 0x00200000;
    pub const has_out_parms: u32 = 0x00400000;
    pub const has_defaults: u32 = 0x00800000;
    pub const net_client: u32 = 0x01000000;
    pub const dll_import: u32 = 0x02000000;
};

/// CLASS_* flags (u32).
pub const class_ = struct {
    pub const abstract: u32 = 0x00000001;
    pub const compiled: u32 = 0x00000002;
    pub const config: u32 = 0x00000004;
    pub const transient: u32 = 0x00000008;
    pub const parsed: u32 = 0x00000010;
    pub const localized: u32 = 0x00000020;
    pub const safe_replace: u32 = 0x00000040;
    pub const native: u32 = 0x00000080;
    pub const no_export: u32 = 0x00000100;
    pub const placeable: u32 = 0x00000200;
    pub const per_object_config: u32 = 0x00000400;
    pub const native_replication: u32 = 0x00000800;
    pub const edit_inline_new: u32 = 0x00001000;
    pub const collapse_categories: u32 = 0x00002000;
    pub const interface: u32 = 0x00004000;
    pub const has_instanced_props: u32 = 0x00200000;
    pub const needs_def_props: u32 = 0x00400000;
    pub const has_components: u32 = 0x00800000;
    pub const hidden: u32 = 0x01000000;
    pub const deprecated: u32 = 0x02000000;
    pub const hide_drop_down: u32 = 0x04000000;
    pub const exported: u32 = 0x08000000;
    pub const intrinsic: u32 = 0x10000000;
    pub const native_only: u32 = 0x20000000;
    pub const per_object_localized: u32 = 0x40000000;
    pub const has_cross_level_refs: u32 = 0x80000000;

    pub const inherit: u32 = transient | config | localized | safe_replace |
        per_object_config | per_object_localized | placeable;
    pub const script_inherit: u32 = inherit | edit_inline_new | collapse_categories;
    pub const recompiler_clear: u32 = inherit | abstract | no_export | native_replication | native;
};

/// STATE_* flags (u32).
pub const state_ = struct {
    pub const editable: u32 = 0x00000001;
    pub const auto: u32 = 0x00000002;
    pub const simulated: u32 = 0x00000004;
    pub const has_locals: u32 = 0x00000008;
};

/// RF_* object flags (u64), the subset used on export-map object_flags.
pub const rf = struct {
    pub const in_singular_func: u64 = 0x0000000000000002;
    pub const state_changed: u64 = 0x0000000000000004;
    pub const debug_serialize: u64 = 0x0000000000000010;
    pub const protected_: u64 = 0x0000000000000100;
    pub const class_default_object: u64 = 0x0000000000000200;
    pub const archetype_object: u64 = 0x0000000000000400;
    pub const transactional: u64 = 0x0000000100000000;
    pub const public_: u64 = 0x0000000400000000;
    pub const need_load: u64 = 0x0000020000000000;
    pub const transient: u64 = 0x0000400000000000;
    pub const standalone: u64 = 0x0008000000000000;
    pub const not_for_client: u64 = 0x0010000000000000;
    pub const not_for_server: u64 = 0x0020000000000000;
    pub const has_stack: u64 = 0x0200000000000000;
    pub const native: u64 = 0x0400000000000000;
};

/// STRUCT_* flags (u32).
pub const struct_ = struct {
    pub const native: u32 = 0x00000001;
    pub const has_components: u32 = 0x00000004;
    pub const transient: u32 = 0x00000008;
    pub const atomic: u32 = 0x00000010;
    pub const immutable: u32 = 0x00000020;
    pub const strict_config: u32 = 0x00000040;
    pub const immutable_when_cooked: u32 = 0x00000080;
    pub const atomic_when_cooked: u32 = 0x00000100;
};

/// Class names for the built-in core property classes (imports).
pub const core_property_classes = [_][]const u8{
    "ArrayProperty",
    "BoolProperty",
    "ByteProperty",
    "ClassProperty",
    "ComponentProperty",
    "Const",
    "DelegateProperty",
    "Enum",
    "FloatProperty",
    "Function",
    "IntProperty",
    "InterfaceProperty",
    "MapProperty",
    "NameProperty",
    "ObjectProperty",
    "ScriptStruct",
    "State",
    "StrProperty",
    "StringProperty",
    "StructProperty",
    "Class",
};
