// Bytecode writer for the UE3 script VM.
//
// On disk, object/property references are serialized as 4-byte package
// indices (ULinkerSave writes ObjectIndices); in memory they are 8-byte
// ScriptPointers. The struct therefore tracks both the on-disk byte count
// (ScriptStorageSize) and the in-memory script size (ScriptBytecodeSize).

const std = @import("std");
const opcodes = @import("opcodes.zig");

pub const Writer = struct {
    allocator: std.mem.Allocator,
    /// On-disk bytecode.
    bytes: std.ArrayList(u8),
    /// In-memory script size (pointers are 8 bytes there).
    mem_size: usize = 0,

    pub fn init(allocator: std.mem.Allocator) Writer {
        return .{
            .allocator = allocator,
            .bytes = std.ArrayList(u8).empty,
        };
    }

    pub fn deinit(self: *Writer) void {
        self.bytes.deinit(self.allocator);
    }

    pub fn toOwnedSlice(self: *Writer) ![]u8 {
        return self.bytes.toOwnedSlice(self.allocator);
    }

    /// Emit a single byte (opcode, cast token, property type, etc).
    pub fn byte(self: *Writer, b: u8) !void {
        try self.bytes.append(self.allocator, b);
        self.mem_size += 1;
    }

    pub fn opcode(self: *Writer, op: opcodes.Expr) !void {
        try self.byte(@intFromEnum(op));
    }

    /// Emit a 2-byte value (CodeSkipSizeType / WORD).
    pub fn word(self: *Writer, v: u16) !void {
        var buf: [2]u8 = undefined;
        std.mem.writeInt(u16, &buf, v, .little);
        try self.bytes.appendSlice(self.allocator, &buf);
        self.mem_size += 2;
    }

    pub fn int32(self: *Writer, v: i32) !void {
        var buf: [4]u8 = undefined;
        std.mem.writeInt(i32, &buf, v, .little);
        try self.bytes.appendSlice(self.allocator, &buf);
        self.mem_size += 4;
    }

    pub fn uint32(self: *Writer, v: u32) !void {
        try self.int32(@bitCast(v));
    }

    pub fn uint64(self: *Writer, v: u64) !void {
        var buf: [8]u8 = undefined;
        std.mem.writeInt(u64, &buf, v, .little);
        try self.bytes.appendSlice(self.allocator, &buf);
        self.mem_size += 8;
    }

    pub fn float(self: *Writer, v: f32) !void {
        try self.uint32(@bitCast(v));
    }

    /// Emit an FName: (name index, instance number), 8 bytes on disk.
    pub fn name(self: *Writer, name_index: u32, number: i32) !void {
        try self.int32(@intCast(name_index));
        try self.int32(number);
    }

    /// Emit a reference to an object/property: 4 bytes on disk, 8 in memory.
    pub fn object(self: *Writer, ref: i32) !void {
        try self.int32(ref);
        self.mem_size += 4; // in-memory ScriptPointer is 8 bytes total
    }

    /// Emit a null-terminated ANSI string constant (EX_StringConst payload).
    pub fn stringConst(self: *Writer, text: []const u8) !void {
        try self.bytes.appendSlice(self.allocator, text);
        try self.bytes.append(self.allocator, 0);
        self.mem_size += text.len + 1;
    }

    pub fn pos(self: *Writer) usize {
        return self.bytes.items.len;
    }

    /// Backpatch a CodeSkipSizeType placeholder with the jump target offset.
    pub fn patchWord(self: *Writer, at: usize, v: u16) void {
        std.mem.writeInt(u16, self.bytes.items[at..][0..2], v, .little);
    }

    /// Overwrite 4 bytes (used to patch object references).
    pub fn patchInt32(self: *Writer, at: usize, v: i32) void {
        std.mem.writeInt(i32, self.bytes.items[at..][0..4], v, .little);
    }
};
