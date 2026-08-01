const std = @import("std");

const Parser = @import("parser.zig");

fn line(comptime fmt: []const u8, args: anytype) void {
    std.debug.print(fmt ++ "\n", args);
}

/// Print a human-readable dump of the parsed package: summary, name map,
/// import map, export map (capped for large packages), and export-data stats.
pub fn generatePackageReport(p: *const Parser) void {
    const s = &p.package_file_summary;

    line("==============================================", .{});
    line("Package Report", .{});
    line("==============================================", .{});
    line("        Filename: {s}", .{s.folder_name.data});
    line("    File Version: {d} (licensee {d})", .{ s.getFileVersion().version, s.getFileVersion().licensee });
    line("    Engine Ver:   {d}", .{s.engine_version});
    line("    Cooker Ver:   {d}", .{s.getCookedContentVersion().version});
    line("    PackageFlags: 0x{X:0>8} ({f})", .{ @as(u32, @bitCast(s.package_flags)), s.package_flags });
    line("    TotalHeader:  {d}", .{s.total_header_size});
    line("    Compression:  {s}", .{if (s.package_flags.store_compressed or s.package_flags.store_fully_compressed) "stored compressed" else "none"});
    line("    CompressionFlags: 0x{X:0>8} ({f})", .{ @as(u32, @bitCast(s.compression_flags)), s.compression_flags });

    line("", .{});
    line("Name Map ({d}):", .{p.name_map.len});
    for (p.name_map, 0..) |entry, i| {
        line("  [{d}] {s} (flags 0x{X})", .{ i, entry.name, entry.flags });
    }

    line("", .{});
    line("Import Map ({d}):", .{p.import_map.len});
    var buf: [512]u8 = undefined;
    for (p.import_map, 0..) |imp, i| {
        line("  [{d}] package: {s}", .{ i, p.formatFName(imp.class_package, &buf) });
        line("       class:   {s}", .{p.formatFName(imp.class_name, &buf)});
        line("       object:  {s}", .{p.formatFName(imp.object_name, &buf)});
        line("       outer:   {d}", .{imp.outer_index});
    }

    line("", .{});
    line("Export Map ({d}):", .{p.export_map.len});
    const max_exports = 25;
    const shown = @min(p.export_map.len, max_exports);
    for (p.export_map[0..shown], 0..shown) |exp, i| {
        line("  [{d}] {s}", .{ i, p.formatFName(exp.object_name, &buf) });
        line("       class: {s}", .{p.resolveClassIndex(exp.class_index, &buf)});
        line("       outer: {s}", .{p.resolvePackageIndex(exp.outer_index, &buf)});
        line("       super: {s}", .{p.resolvePackageIndex(exp.super_index, &buf)});
        line("       arch:  {s}", .{p.resolvePackageIndex(exp.archetype_index, &buf)});
        line("       flags: 0x{X:0>16}  export_flags: 0x{X:0>8}  pkg_flags: 0x{X:0>8}", .{ exp.object_flags, exp.export_flags, exp.package_flags });
        line("       serial: offset {d}, size {d}", .{ exp.serial_offset, exp.serial_size });
    }
    if (p.export_map.len > max_exports) {
        line("  ... ({d} more)", .{p.export_map.len - max_exports});
    }

    var total_bytes: u64 = 0;
    var with_data: usize = 0;
    for (p.export_data) |blob| {
        if (blob.len > 0) with_data += 1;
        total_bytes += blob.len;
    }
    line("", .{});
    line("Export data: {d}/{d} exports have data, {d} total bytes", .{ with_data, p.export_data.len, total_bytes });
}
