pub const CMarshalRow = struct {
    entry_count: u32,
    entry_list: ?*CMarshalEntry,
    entry_tail: ?*CMarshalEntry,
    entry_pool: ?*CMarshalEntry,
};

pub const CMarshalRowSet = struct {
    rows: []CMarshalRow,
};

pub const CMarshalEntry = struct {
    data: union {
        dw_number: u32,
        n_number: i32,
        f_number: f32,
        d_number: f64,
        qw_number: u64,
        wz_local: [4]u8,
        wz_pointer: ?*const u8,
        by_data: ?*const u8,
        row_set: ?*CMarshalRowSet,
    },
    next: ?*CMarshalEntry,
    size: u16,
};

pub const CMscEntry = struct {
    next: ?*CMscEntry,
};

pub const CPackPacketNET = struct {
    header: union {
        fields: struct {
            size: u16,
            extended: u8,
            flags: u8,
        },
        all: u32,
    },
    data: [0x7fe]u8,
};

pub const CPackPacket = struct {
    next: ?*CPackPacket,
    net: CPackPacketNET,
    source_func: u32,
};

pub const CPackage = struct {
    base: CMscEntry,

    packets: ?*CPackPacket,
    used: u32,
    place: u32,
    current: ?*CPackPacket,
    cur_place: u32,
    first_alloc: bool,
    flags: u8,
    extended: u8,
    pby_extended: [0xc]u8,
    encoding: u8,
    db_action_func: u32,
};
