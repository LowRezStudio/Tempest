const std = @import("std");
const memory = @import("root").memory;
const Lua = @import("luajit").Lua;

const windows = std.os.windows;
const Module = memory.Module;

// to-do: use tom's memory module
extern "kernel32" fn GetModuleHandleA(lpModuleName: ?[*:0]const u8) callconv(.winapi) windows.HMODULE;

export fn op_memory_module_init(name: ?[*:0]const u8) *Module {
    return Module.init(name) orelse null;
}

export fn op_memory_module_base() usize {
    return @intFromPtr(GetModuleHandleA(null));
}

export fn op_memory_read_u8(address: usize) u8 {
    return @as(*u8, @ptrFromInt(address)).*;
}

export fn op_memory_read_u16(address: usize) u16 {
    return @as(*u16, @ptrFromInt(address)).*;
}

export fn op_memory_read_u32(address: usize) u32 {
    return @as(*u32, @ptrFromInt(address)).*;
}

export fn op_memory_read_usize(address: usize) usize {
    return @as(*usize, @ptrFromInt(address)).*;
}

export fn op_memory_read_i8(address: usize) i8 {
    return @as(*i8, @ptrFromInt(address)).*;
}

export fn op_memory_read_i16(address: usize) i16 {
    return @as(*i16, @ptrFromInt(address)).*;
}

export fn op_memory_read_i32(address: usize) i32 {
    return @as(*i32, @ptrFromInt(address)).*;
}

export fn op_memory_read_isize(address: usize) isize {
    return @as(*isize, @ptrFromInt(address)).*;
}

export fn op_memory_write_u8(address: usize, value: u8) void {
    @as(*u8, @ptrFromInt(address)).* = value;
}

export fn op_memory_write_u16(address: usize, value: u16) void {
    @as(*u16, @ptrFromInt(address)).* = value;
}

export fn op_memory_write_u32(address: usize, value: u32) void {
    @as(*u32, @ptrFromInt(address)).* = value;
}

export fn op_memory_write_usize(address: usize, value: usize) void {
    @as(*usize, @ptrFromInt(address)).* = value;
}

export fn op_memory_write_i8(address: usize, value: i8) void {
    @as(*i8, @ptrFromInt(address)).* = value;
}

export fn op_memory_write_i16(address: usize, value: i16) void {
    @as(*i16, @ptrFromInt(address)).* = value;
}

export fn op_memory_write_i32(address: usize, value: i32) void {
    @as(*i32, @ptrFromInt(address)).* = value;
}

export fn op_memory_write_isize(address: usize, value: isize) void {
    @as(*isize, @ptrFromInt(address)).* = value;
}

pub fn init(lua: *Lua) !void {
    try lua.doString(@embedFile("lib.lua"));
}
