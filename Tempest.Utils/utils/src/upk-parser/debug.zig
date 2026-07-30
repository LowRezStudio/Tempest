const std = @import("std");

const Parser = @import("parser.zig");

pub fn generatePackageReport(p: Parser) void {
    std.debug.print("********************************************\n", .{});
    std.debug.print("Package '{s}' Summary\n", .{p.package_file_summary.folder_name.data});
    std.debug.print("--------------------------------------------\n", .{});

    std.debug.print("\t         Filename: {s}\n", .{p.package_file_summary.folder_name.data});
    std.debug.print("\t     File Version: {d}\n", .{p.package_file_summary.getFileVersion().version});
    std.debug.print("\t   Engine Version: {d}\n", .{p.package_file_summary.engine_version});
    std.debug.print("\t   Cooker Version: {d}\n", .{p.package_file_summary.getCookedContentVersion().version});
    std.debug.print("\t     PackageFlags: 0x{X:0>8} ({f})\n", .{ @as(u32, @bitCast(p.package_file_summary.package_flags)), p.package_file_summary.package_flags });
    std.debug.print("\t        NameCount: {d}\n", .{p.package_file_summary.name_count});
    std.debug.print("\t       NameOffset: {d}\n", .{p.package_file_summary.name_offset});
    std.debug.print("\t      ImportCount: {d}\n", .{p.package_file_summary.import_count});
    std.debug.print("\t     ImportOffset: {d}\n", .{p.package_file_summary.import_offset});
    std.debug.print("\t      ExportCount: {d}\n", .{p.package_file_summary.export_count});
    std.debug.print("\t     ExportOffset: {d}\n", .{p.package_file_summary.export_offset});
}
