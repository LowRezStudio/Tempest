const std = @import("std");

const unreal = @import("unreal.zig");
const FPackageFileSummary = unreal.FPackageFileSummary;

const Parser = @This();

allocator: std.mem.Allocator,
file_buffer: []u8,

package_file_summary: FPackageFileSummary,

pub fn init(io: std.Io, filepath: []const u8, allocator: std.mem.Allocator) !Parser {
    const file_buffer = try std.Io.Dir.readFileAlloc(std.Io.Dir.cwd(), io, filepath, allocator, .unlimited);

    return Parser{
        .allocator = allocator,
        .file_buffer = file_buffer,

        .package_file_summary = undefined,
    };
}

pub fn deinit(self: *Parser) void {
    self.allocator.free(self.file_buffer);
    self.package_file_summary.deinit(self.allocator);
}

pub fn parse(self: *Parser) !void {
    var fr: std.Io.Reader = .fixed(self.file_buffer);
    const reader = &fr;

    self.package_file_summary = try FPackageFileSummary.take(reader, self.allocator);
}
