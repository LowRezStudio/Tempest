const std = @import("std");
const builtin = @import("builtin");
const arch = builtin.cpu.arch;

pub const THISCALL: std.builtin.CallingConvention = if (arch == .x86) .x86_thiscall else .x86_fastcall;
