//! Flag constants as used by Paladins 8.2 (UE3 build 10897). Values were
//! cross-checked against the DWARF enums in the dedicated-server binary.

pub const PACKAGE_FILE_TAG: u32 = 0x9E2A_83C1;
pub const PACKAGE_FILE_TAG_SWAPPED: u32 = 0xC183_2A9E;
/// `LOADING_COMPRESSION_CHUNK_SIZE`
pub const LOADING_COMPRESSION_CHUNK_SIZE: u32 = 0x20000;

/// Package file version written by the game and its cooker.
pub const PACKAGE_FILE_VERSION: u16 = 893;
pub const PACKAGE_FILE_LICENSEE_VERSION: u16 = 0;
/// `GEngineVersion` of the shipped build.
pub const ENGINE_VERSION: u32 = 10897;
/// `GPackageFileCookedContentVersion` seen in shipped packages.
pub const COOKED_CONTENT_VERSION: u32 = 0x0600_008E;

pub mod compression {
    pub const NONE: u32 = 0x0;
    pub const ZLIB: u32 = 0x1;
    pub const LZO: u32 = 0x2;
    pub const LZX: u32 = 0x4;
    pub const BIAS_MEMORY: u32 = 0x10;
    pub const BIAS_SPEED: u32 = 0x20;
    pub const FORCE_PPU_DECOMPRESS_ZLIB: u32 = 0x80;
    pub const NO_STATS: u32 = 0x100;
    /// Hi-Rez: compressed chunk payloads are XOR-ed with `OBSCURE_KEY`.
    pub const OBSCURED: u32 = 0x200;
    pub const TYPE_MASK: u32 = ZLIB | LZO | LZX;
    pub const OBSCURE_KEY: u8 = 0x2A;
}

pub mod pkg {
    pub const ALLOW_DOWNLOAD: u32 = 0x0000_0001;
    pub const CLIENT_OPTIONAL: u32 = 0x0000_0002;
    pub const SERVER_SIDE_ONLY: u32 = 0x0000_0004;
    pub const COOKED: u32 = 0x0000_0008;
    pub const UNSECURE: u32 = 0x0000_0010;
    pub const SAVED_WITH_NEWER_VERSION: u32 = 0x0000_0020;
    /// Hi-Rez (`PKG_TG_CompressedObscured`).
    pub const TG_COMPRESSED_OBSCURED: u32 = 0x0000_0100;
    pub const NEED: u32 = 0x0000_8000;
    pub const COMPILING: u32 = 0x0001_0000;
    pub const CONTAINS_MAP: u32 = 0x0002_0000;
    pub const TRASH: u32 = 0x0004_0000;
    pub const DISALLOW_LAZY_LOADING: u32 = 0x0008_0000;
    pub const PLAY_IN_EDITOR: u32 = 0x0010_0000;
    pub const CONTAINS_SCRIPT: u32 = 0x0020_0000;
    pub const CONTAINS_DEBUG_INFO: u32 = 0x0040_0000;
    pub const REQUIRE_IMPORTS_ALREADY_LOADED: u32 = 0x0080_0000;
    pub const STORE_COMPRESSED: u32 = 0x0200_0000;
    pub const STORE_FULLY_COMPRESSED: u32 = 0x0400_0000;
    pub const CONTAINS_FACEFX_DATA: u32 = 0x1000_0000;
    pub const NO_EXPORT_ALLOWED: u32 = 0x2000_0000;
    pub const STRIPPED_SOURCE: u32 = 0x4000_0000;
    pub const FILTER_EDITOR_ONLY: u32 = 0x8000_0000;
}

pub mod export {
    pub const NONE: u32 = 0;
    pub const FORCED_EXPORT: u32 = 0x1;
    pub const SCRIPT_PATCHER_EXPORT: u32 = 0x2;
    pub const MEMBER_FIELD_PATCH_PENDING: u32 = 0x4;
}

/// `EObjectFlags` (64-bit).
pub mod rf {
    pub const IN_SINGULAR_FUNC: u64 = 0x0000_0000_0000_0002;
    pub const STATE_CHANGED: u64 = 0x0000_0000_0000_0004;
    pub const DEBUG_POST_LOAD: u64 = 0x0000_0000_0000_0008;
    pub const DEBUG_SERIALIZE: u64 = 0x0000_0000_0000_0010;
    pub const DEBUG_FINISH_DESTROYED: u64 = 0x0000_0000_0000_0020;
    pub const ED_SELECTED: u64 = 0x0000_0000_0000_0040;
    pub const ZOMBIE_COMPONENT: u64 = 0x0000_0000_0000_0080;
    pub const PROTECTED: u64 = 0x0000_0000_0000_0100;
    pub const CLASS_DEFAULT_OBJECT: u64 = 0x0000_0000_0000_0200;
    pub const ARCHETYPE_OBJECT: u64 = 0x0000_0000_0000_0400;
    pub const FORCE_TAG_EXP: u64 = 0x0000_0000_0000_0800;
    pub const TOKEN_STREAM_ASSEMBLED: u64 = 0x0000_0000_0000_1000;
    pub const MISALIGNED_OBJECT: u64 = 0x0000_0000_0000_2000;
    pub const ROOT_SET: u64 = 0x0000_0000_0000_4000;
    pub const BEGIN_DESTROYED: u64 = 0x0000_0000_0000_8000;
    pub const FINISH_DESTROYED: u64 = 0x0000_0000_0001_0000;
    pub const DEBUG_BEGIN_DESTROYED: u64 = 0x0000_0000_0002_0000;
    pub const MARKED_BY_COOKER: u64 = 0x0000_0000_0004_0000;
    pub const LOCALIZED_RESOURCE: u64 = 0x0000_0000_0008_0000;
    pub const INITIALIZED_PROPS: u64 = 0x0000_0000_0010_0000;
    pub const PENDING_FIELD_PATCHES: u64 = 0x0000_0000_0020_0000;
    pub const IS_CROSS_LEVEL_REFERENCED: u64 = 0x0000_0000_0040_0000;
    pub const SAVED: u64 = 0x0000_0000_8000_0000;
    pub const TRANSACTIONAL: u64 = 0x0000_0001_0000_0000;
    pub const UNREACHABLE: u64 = 0x0000_0002_0000_0000;
    pub const PUBLIC: u64 = 0x0000_0004_0000_0000;
    pub const TAG_IMP: u64 = 0x0000_0008_0000_0000;
    pub const TAG_EXP: u64 = 0x0000_0010_0000_0000;
    pub const OBSOLETE: u64 = 0x0000_0020_0000_0000;
    pub const TAG_GARBAGE: u64 = 0x0000_0040_0000_0000;
    pub const DISREGARD_FOR_GC: u64 = 0x0000_0080_0000_0000;
    pub const PER_OBJECT_LOCALIZED: u64 = 0x0000_0100_0000_0000;
    pub const NEED_LOAD: u64 = 0x0000_0200_0000_0000;
    pub const ASYNC_LOADING: u64 = 0x0000_0400_0000_0000;
    pub const NEED_POST_LOAD_SUBOBJECTS: u64 = 0x0000_0800_0000_0000;
    pub const SUPPRESS: u64 = 0x0000_1000_0000_0000;
    pub const IN_END_STATE: u64 = 0x0000_2000_0000_0000;
    pub const TRANSIENT: u64 = 0x0000_4000_0000_0000;
    pub const COOKED: u64 = 0x0000_8000_0000_0000;
    pub const LOAD_FOR_CLIENT: u64 = 0x0001_0000_0000_0000;
    pub const LOAD_FOR_SERVER: u64 = 0x0002_0000_0000_0000;
    pub const LOAD_FOR_EDIT: u64 = 0x0004_0000_0000_0000;
    pub const STANDALONE: u64 = 0x0008_0000_0000_0000;
    pub const NOT_FOR_CLIENT: u64 = 0x0010_0000_0000_0000;
    pub const NOT_FOR_SERVER: u64 = 0x0020_0000_0000_0000;
    pub const NOT_FOR_EDIT: u64 = 0x0040_0000_0000_0000;
    pub const NEED_POST_LOAD: u64 = 0x0100_0000_0000_0000;
    pub const HAS_STACK: u64 = 0x0200_0000_0000_0000;
    pub const NATIVE: u64 = 0x0400_0000_0000_0000;
    pub const MARKED: u64 = 0x0800_0000_0000_0000;
    pub const ERROR_SHUTDOWN: u64 = 0x1000_0000_0000_0000;
    pub const PENDING_KILL: u64 = 0x2000_0000_0000_0000;
    pub const MARKED_BY_COOKER_TEMP: u64 = 0x4000_0000_0000_0000;
    pub const COOKED_STARTUP_OBJECT: u64 = 0x8000_0000_0000_0000;

    pub const CONTEXT_FLAGS: u64 = NOT_FOR_CLIENT | NOT_FOR_SERVER | NOT_FOR_EDIT;
    pub const LOAD_CONTEXT_FLAGS: u64 = LOAD_FOR_CLIENT | LOAD_FOR_SERVER | LOAD_FOR_EDIT;
    pub const LOAD: u64 = CONTEXT_FLAGS
        | LOAD_CONTEXT_FLAGS
        | PUBLIC
        | STANDALONE
        | NATIVE
        | OBSOLETE
        | PROTECTED
        | TRANSACTIONAL
        | HAS_STACK
        | PER_OBJECT_LOCALIZED
        | CLASS_DEFAULT_OBJECT
        | ARCHETYPE_OBJECT
        | LOCALIZED_RESOURCE;
}

/// `EClassFlags`.
pub mod class {
    pub const ABSTRACT: u32 = 0x0000_0001;
    pub const COMPILED: u32 = 0x0000_0002;
    pub const CONFIG: u32 = 0x0000_0004;
    pub const TRANSIENT: u32 = 0x0000_0008;
    pub const PARSED: u32 = 0x0000_0010;
    pub const LOCALIZED: u32 = 0x0000_0020;
    pub const SAFE_REPLACE: u32 = 0x0000_0040;
    pub const NATIVE: u32 = 0x0000_0080;
    pub const NO_EXPORT: u32 = 0x0000_0100;
    pub const PLACEABLE: u32 = 0x0000_0200;
    pub const PER_OBJECT_CONFIG: u32 = 0x0000_0400;
    pub const NATIVE_REPLICATION: u32 = 0x0000_0800;
    pub const EDIT_INLINE_NEW: u32 = 0x0000_1000;
    pub const COLLAPSE_CATEGORIES: u32 = 0x0000_2000;
    pub const INTERFACE: u32 = 0x0000_4000;
    pub const IS_A_UPROPERTY: u32 = 0x0000_8000;
    pub const IS_A_UOBJECT_PROPERTY: u32 = 0x0001_0000;
    pub const IS_A_UBOOL_PROPERTY: u32 = 0x0002_0000;
    pub const IS_A_USTATE: u32 = 0x0004_0000;
    pub const IS_A_UFUNCTION: u32 = 0x0008_0000;
    pub const IS_A_USTRUCT_PROPERTY: u32 = 0x0010_0000;
    pub const HAS_INSTANCED_PROPS: u32 = 0x0020_0000;
    pub const NEEDS_DEF_PROPS: u32 = 0x0040_0000;
    pub const HAS_COMPONENTS: u32 = 0x0080_0000;
    pub const HIDDEN: u32 = 0x0100_0000;
    pub const DEPRECATED: u32 = 0x0200_0000;
    pub const HIDE_DROP_DOWN: u32 = 0x0400_0000;
    pub const EXPORTED: u32 = 0x0800_0000;
    pub const INTRINSIC: u32 = 0x1000_0000;
    pub const NATIVE_ONLY: u32 = 0x2000_0000;
    pub const PER_OBJECT_LOCALIZED: u32 = 0x4000_0000;
    pub const HAS_CROSS_LEVEL_REFS: u32 = 0x8000_0000;

    pub const INHERIT: u32 = TRANSIENT
        | CONFIG
        | LOCALIZED
        | SAFE_REPLACE
        | PER_OBJECT_CONFIG
        | PER_OBJECT_LOCALIZED
        | PLACEABLE
        | IS_A_UPROPERTY
        | IS_A_UOBJECT_PROPERTY
        | IS_A_UBOOL_PROPERTY
        | IS_A_USTATE
        | IS_A_UFUNCTION
        | IS_A_USTRUCT_PROPERTY
        | HAS_COMPONENTS
        | DEPRECATED
        | HAS_INSTANCED_PROPS
        | HAS_CROSS_LEVEL_REFS;
    pub const RECOMPILER_CLEAR: u32 =
        INHERIT | ABSTRACT | NO_EXPORT | NATIVE_REPLICATION | NATIVE;
    pub const SCRIPT_INHERIT: u32 = INHERIT | EDIT_INLINE_NEW | COLLAPSE_CATEGORIES;
}

/// `EFunctionFlags`.
pub mod func {
    pub const FINAL: u32 = 0x0000_0001;
    pub const DEFINED: u32 = 0x0000_0002;
    pub const ITERATOR: u32 = 0x0000_0004;
    pub const LATENT: u32 = 0x0000_0008;
    pub const PRE_OPERATOR: u32 = 0x0000_0010;
    pub const SINGULAR: u32 = 0x0000_0020;
    pub const NET: u32 = 0x0000_0040;
    pub const NET_RELIABLE: u32 = 0x0000_0080;
    pub const SIMULATED: u32 = 0x0000_0100;
    pub const EXEC: u32 = 0x0000_0200;
    pub const NATIVE: u32 = 0x0000_0400;
    pub const EVENT: u32 = 0x0000_0800;
    pub const OPERATOR: u32 = 0x0000_1000;
    pub const STATIC: u32 = 0x0000_2000;
    pub const HAS_OPTIONAL_PARMS: u32 = 0x0000_4000;
    pub const CONST: u32 = 0x0000_8000;
    pub const PUBLIC: u32 = 0x0002_0000;
    pub const PRIVATE: u32 = 0x0004_0000;
    pub const PROTECTED: u32 = 0x0008_0000;
    pub const DELEGATE: u32 = 0x0010_0000;
    pub const NET_SERVER: u32 = 0x0020_0000;
    pub const HAS_OUT_PARMS: u32 = 0x0040_0000;
    pub const HAS_DEFAULTS: u32 = 0x0080_0000;
    pub const NET_CLIENT: u32 = 0x0100_0000;
    pub const DLL_IMPORT: u32 = 0x0200_0000;
    /// Hi-Rez: `FUNC_PComNetApproved`.
    pub const PCOM_NET_APPROVED: u32 = 0x2000_0000;
    /// Hi-Rez: `FUNC_PComServerCmd`.
    pub const PCOM_SERVER_CMD: u32 = 0x4000_0000;

    pub const FUNC_INHERIT: u32 = EXEC | EVENT;
    pub const FUNC_OVERRIDE_MATCH: u32 =
        EXEC | FINAL | LATENT | PRE_OPERATOR | ITERATOR | STATIC | PUBLIC | PROTECTED | CONST;
    pub const NET_FUNC_FLAGS: u32 = NET | NET_RELIABLE | NET_SERVER | NET_CLIENT;
}

/// `EStateFlags`.
pub mod state {
    pub const EDITABLE: u32 = 0x1;
    pub const AUTO: u32 = 0x2;
    pub const SIMULATED: u32 = 0x4;
    pub const HAS_LOCALS: u32 = 0x8;
}

/// `EStructFlags`.
pub mod strct {
    pub const NATIVE: u32 = 0x1;
    pub const EXPORT: u32 = 0x2;
    pub const HAS_COMPONENTS: u32 = 0x4;
    pub const TRANSIENT: u32 = 0x8;
    pub const ATOMIC: u32 = 0x10;
    pub const IMMUTABLE: u32 = 0x20;
    pub const STRICT_CONFIG: u32 = 0x40;
    pub const IMMUTABLE_WHEN_COOKED: u32 = 0x80;
    pub const ATOMIC_WHEN_COOKED: u32 = 0x100;
    pub const INHERIT: u32 = HAS_COMPONENTS | ATOMIC | ATOMIC_WHEN_COOKED | STRICT_CONFIG;
}

/// `EPropertyFlags` (64-bit, `CPF_*`).
pub mod cpf {
    pub const EDIT: u64 = 0x0000_0000_0000_0001;
    pub const CONST: u64 = 0x0000_0000_0000_0002;
    pub const INPUT: u64 = 0x0000_0000_0000_0004;
    pub const EXPORT_OBJECT: u64 = 0x0000_0000_0000_0008;
    pub const OPTIONAL_PARM: u64 = 0x0000_0000_0000_0010;
    pub const NET: u64 = 0x0000_0000_0000_0020;
    pub const EDIT_FIXED_SIZE: u64 = 0x0000_0000_0000_0040;
    pub const PARM: u64 = 0x0000_0000_0000_0080;
    pub const OUT_PARM: u64 = 0x0000_0000_0000_0100;
    pub const SKIP_PARM: u64 = 0x0000_0000_0000_0200;
    pub const RETURN_PARM: u64 = 0x0000_0000_0000_0400;
    pub const COERCE_PARM: u64 = 0x0000_0000_0000_0800;
    pub const NATIVE: u64 = 0x0000_0000_0000_1000;
    pub const TRANSIENT: u64 = 0x0000_0000_0000_2000;
    pub const CONFIG: u64 = 0x0000_0000_0000_4000;
    pub const LOCALIZED: u64 = 0x0000_0000_0000_8000;
    pub const EDIT_CONST: u64 = 0x0000_0000_0002_0000;
    pub const GLOBAL_CONFIG: u64 = 0x0000_0000_0004_0000;
    pub const COMPONENT: u64 = 0x0000_0000_0008_0000;
    pub const ALWAYS_INIT: u64 = 0x0000_0000_0010_0000;
    pub const DUPLICATE_TRANSIENT: u64 = 0x0000_0000_0020_0000;
    pub const NEED_CTOR_LINK: u64 = 0x0000_0000_0040_0000;
    pub const NO_EXPORT: u64 = 0x0000_0000_0080_0000;
    pub const NO_IMPORT: u64 = 0x0000_0000_0100_0000;
    pub const NO_CLEAR: u64 = 0x0000_0000_0200_0000;
    pub const EDIT_INLINE: u64 = 0x0000_0000_0400_0000;
    pub const EDIT_INLINE_USE: u64 = 0x0000_0000_1000_0000;
    pub const DEPRECATED: u64 = 0x0000_0000_2000_0000;
    pub const DATA_BINDING: u64 = 0x0000_0000_4000_0000;
    pub const SERIALIZE_TEXT: u64 = 0x0000_0000_8000_0000;
    pub const REP_NOTIFY: u64 = 0x0000_0001_0000_0000;
    pub const INTERP: u64 = 0x0000_0002_0000_0000;
    pub const NON_TRANSACTIONAL: u64 = 0x0000_0004_0000_0000;
    pub const EDITOR_ONLY: u64 = 0x0000_0008_0000_0000;
    pub const NOT_FOR_CONSOLE: u64 = 0x0000_0010_0000_0000;
    pub const REP_RETRY: u64 = 0x0000_0020_0000_0000;
    pub const PRIVATE_WRITE: u64 = 0x0000_0040_0000_0000;
    pub const PROTECTED_WRITE: u64 = 0x0000_0080_0000_0000;
    pub const ARCHETYPE_PROPERTY: u64 = 0x0000_0100_0000_0000;
    pub const EDIT_HIDE: u64 = 0x0000_0200_0000_0000;
    pub const EDIT_TEXT_BOX: u64 = 0x0000_0400_0000_0000;
    pub const CROSS_LEVEL_PASSIVE: u64 = 0x0000_1000_0000_0000;
    /// Hi-Rez: set on `safeobject<Class>` properties.
    pub const SAFE_OBJECT: u64 = 0x0000_4000_0000_0000;
    pub const CROSS_LEVEL_ACTIVE: u64 = 0x0000_2000_0000_0000;

    pub const PARM_FLAGS: u64 =
        OPTIONAL_PARM | PARM | OUT_PARM | SKIP_PARM | RETURN_PARM | COERCE_PARM;
    pub const PROPAGATE_FROM_STRUCT: u64 = CONST | NATIVE | TRANSIENT;
    pub const PROPAGATE_TO_ARRAY_INNER: u64 = EXPORT_OBJECT
        | EDIT_INLINE
        | EDIT_INLINE_USE
        | LOCALIZED
        | COMPONENT
        | CONFIG
        | ALWAYS_INIT
        | EDIT_CONST
        | DEPRECATED
        | SERIALIZE_TEXT
        | EDITOR_ONLY
        | NOT_FOR_CONSOLE
        | ARCHETYPE_PROPERTY
        | CROSS_LEVEL_PASSIVE
        | CROSS_LEVEL_ACTIVE;
}

/// Decode a flag word into `NAME|NAME` form using a table.
pub fn describe(value: u64, table: &[(u64, &str)]) -> String {
    let mut parts = Vec::new();
    let mut rest = value;
    for &(bit, name) in table {
        if value & bit == bit && bit != 0 {
            parts.push(name.to_string());
            rest &= !bit;
        }
    }
    if rest != 0 {
        parts.push(format!("{rest:#x}"));
    }
    if parts.is_empty() {
        "0".to_string()
    } else {
        parts.join("|")
    }
}

pub const OBJECT_FLAG_NAMES: &[(u64, &str)] = &[
    (rf::PROTECTED, "Protected"),
    (rf::CLASS_DEFAULT_OBJECT, "ClassDefaultObject"),
    (rf::ARCHETYPE_OBJECT, "ArchetypeObject"),
    (rf::FORCE_TAG_EXP, "ForceTagExp"),
    (rf::LOCALIZED_RESOURCE, "LocalizedResource"),
    (rf::TRANSACTIONAL, "Transactional"),
    (rf::PUBLIC, "Public"),
    (rf::OBSOLETE, "Obsolete"),
    (rf::PER_OBJECT_LOCALIZED, "PerObjectLocalized"),
    (rf::TRANSIENT, "Transient"),
    (rf::COOKED, "Cooked"),
    (rf::LOAD_FOR_CLIENT, "LoadForClient"),
    (rf::LOAD_FOR_SERVER, "LoadForServer"),
    (rf::LOAD_FOR_EDIT, "LoadForEdit"),
    (rf::STANDALONE, "Standalone"),
    (rf::NOT_FOR_CLIENT, "NotForClient"),
    (rf::NOT_FOR_SERVER, "NotForServer"),
    (rf::NOT_FOR_EDIT, "NotForEdit"),
    (rf::HAS_STACK, "HasStack"),
    (rf::NATIVE, "Native"),
    (rf::MARKED, "Marked"),
    (rf::COOKED_STARTUP_OBJECT, "CookedStartupObject"),
];

pub const PACKAGE_FLAG_NAMES: &[(u64, &str)] = &[
    (pkg::ALLOW_DOWNLOAD as u64, "AllowDownload"),
    (pkg::CLIENT_OPTIONAL as u64, "ClientOptional"),
    (pkg::SERVER_SIDE_ONLY as u64, "ServerSideOnly"),
    (pkg::COOKED as u64, "Cooked"),
    (pkg::UNSECURE as u64, "Unsecure"),
    (pkg::TG_COMPRESSED_OBSCURED as u64, "CompressedObscured"),
    (pkg::NEED as u64, "Need"),
    (pkg::COMPILING as u64, "Compiling"),
    (pkg::CONTAINS_MAP as u64, "ContainsMap"),
    (pkg::TRASH as u64, "Trash"),
    (pkg::DISALLOW_LAZY_LOADING as u64, "DisallowLazyLoading"),
    (pkg::PLAY_IN_EDITOR as u64, "PlayInEditor"),
    (pkg::CONTAINS_SCRIPT as u64, "ContainsScript"),
    (pkg::CONTAINS_DEBUG_INFO as u64, "ContainsDebugInfo"),
    (pkg::REQUIRE_IMPORTS_ALREADY_LOADED as u64, "RequireImportsAlreadyLoaded"),
    (pkg::STORE_COMPRESSED as u64, "StoreCompressed"),
    (pkg::STORE_FULLY_COMPRESSED as u64, "StoreFullyCompressed"),
    (pkg::CONTAINS_FACEFX_DATA as u64, "ContainsFaceFXData"),
    (pkg::NO_EXPORT_ALLOWED as u64, "NoExportAllowed"),
    (pkg::STRIPPED_SOURCE as u64, "StrippedSource"),
    (pkg::FILTER_EDITOR_ONLY as u64, "FilterEditorOnly"),
];
