const std = @import("std");

const unreal = @import("unreal.zig");
const property = @import("property.zig");
const Parser = @import("Parser.zig");

fn line(comptime fmt: []const u8, args: anytype) void {
    std.debug.print(fmt ++ "\n", args);
}

/// Cap on how many exports have their property streams shown in -report.
const max_property_exports = 6;

fn formatFNameCtx(p: *const anyopaque, name: unreal.FName, buf: []u8) []const u8 {
    const parser: *const Parser = @ptrCast(@alignCast(p));
    return parser.formatFName(name, buf);
}

fn resolveObjectCtx(p: *const anyopaque, index: i32, buf: []u8) []const u8 {
    const parser: *const Parser = @ptrCast(@alignCast(p));
    if (index == 0) return "None";
    return parser.resolvePackageIndex(index, buf);
}

fn propertyCtx(p: *const Parser) property.Ctx {
    return .{
        .ctx = p,
        .allocator = p.allocator,
        .formatFName = &formatFNameCtx,
        .resolveObject = &resolveObjectCtx,
    };
}

/// Print an already-parsed export's property stream. Takes ownership of
/// `result.properties` (frees it).
fn printExportPropertiesResult(
    p: *const Parser,
    export_index: usize,
    result: property.ParseResult,
    ctx: *const property.Ctx,
    counters: *property.SkipCounter,
) void {
    defer property.deinitProperties(result.properties, p.allocator);
    const blob = p.export_data[export_index];
    const exp = p.export_map[export_index];

    var buf: [512]u8 = undefined;
    line("  --- export {d}: {s} (class {s}, {d} bytes) ---", .{
        export_index,
        p.formatFName(exp.object_name, &buf),
        p.resolveClassIndex(exp.class_index, &buf),
        blob.len,
    });

    line("      net_index: {d}", .{result.net_index});

    // A truncated stream is by definition not a valid property stream: its
    // partial tags may be native-serialized bytes misread as properties, so
    // don't print or count them.
    if (result.truncated) {
        line("      (property stream truncated)", .{});
        return;
    }
    if (result.properties.len == 0) {
        line("      (no tagged properties)", .{});
    } else {
        property.printProperties(result.properties, ctx, 1);
        counters.collect(result.properties, ctx);
    }
    if (blob.len > 4 + result.property_bytes) {
        line("      ({d} bytes of non-property data follow)", .{blob.len - 4 - result.property_bytes});
    }
}

/// Print the sorted summary of skipped (non-core) property types.
fn printSkipSummary(p: *const Parser, counters: *property.SkipCounter) void {
    const SkipEntry = struct { name: []const u8, count: u32 };
    var skip_entries = std.ArrayList(SkipEntry).initCapacity(p.allocator, 0) catch return;
    defer skip_entries.deinit(p.allocator);
    var it = counters.map.iterator();
    while (it.next()) |entry| {
        skip_entries.append(p.allocator, .{ .name = entry.key_ptr.*, .count = entry.value_ptr.* }) catch {};
    }
    std.sort.block(SkipEntry, skip_entries.items, {}, struct {
        fn lessThan(_: void, a: SkipEntry, b: SkipEntry) bool {
            return a.count > b.count;
        }
    }.lessThan);
    line("", .{});
    line("Skipped / non-core property types encountered:", .{});
    if (skip_entries.items.len == 0) {
        line("  (none)", .{});
    }
    for (skip_entries.items) |entry| {
        line("  {s}: {d}", .{ entry.name, entry.count });
    }
}

/// Parse and print the property stream of a single export (`-props <index>`).
pub fn printExportProperties(p: *const Parser, export_index: usize) void {
    if (export_index >= p.export_data.len) {
        line("error: export index {d} out of range ({d} exports)", .{ export_index, p.export_data.len });
        return;
    }
    var counters = property.SkipCounter.init(p.allocator);
    defer counters.deinit();
    const ctx = propertyCtx(p);
    const base: usize = @intCast(@max(p.export_map[export_index].serial_offset, 0));
    const result = property.parseExport(p.export_data[export_index], base, &ctx, p.allocator) catch {
        line("  (property parse failed)", .{});
        return;
    };
    printExportPropertiesResult(p, export_index, result, &ctx, &counters);
    printSkipSummary(p, &counters);
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

    // Property streams: show the first few exports that have a non-empty
    // property list; if none do, show the first data-bearing export.
    var counters = property.SkipCounter.init(p.allocator);
    defer counters.deinit();
    var prop_shown: usize = 0;
    var first_thin: ?usize = null;
    line("", .{});
    line("Properties ({d} exports shown):", .{max_property_exports});
    var i: usize = 0;
    while (i < p.export_data.len and prop_shown < max_property_exports) : (i += 1) {
        const blob = p.export_data[i];
        if (blob.len < 4) continue;
        // Exports with ClassIndex 0 are UClass/Function/ScriptStruct definitions,
        // serialized in a different (non-tagged-property) format.
        if (p.export_map[i].class_index == 0) continue;
        const ctx = propertyCtx(p);
        const base: usize = @intCast(@max(p.export_map[i].serial_offset, 0));
        const result = property.parseExport(blob, base, &ctx, p.allocator) catch continue;
        if (result.truncated or result.properties.len == 0) {
            // Not a clean property stream (native/class data, or no properties):
            // remember the first one as a fallback, but don't count toward the cap.
            property.deinitProperties(result.properties, p.allocator);
            if (first_thin == null) first_thin = i;
            continue;
        }
        prop_shown += 1;
        printExportPropertiesResult(p, i, result, &ctx, &counters);
    }
    if (prop_shown == 0) {
        if (first_thin) |fi| {
            const ctx = propertyCtx(p);
            const base: usize = @intCast(@max(p.export_map[fi].serial_offset, 0));
            const result = property.parseExport(p.export_data[fi], base, &ctx, p.allocator) catch {
                line("  (no parseable property streams)", .{});
                return;
            };
            printExportPropertiesResult(p, fi, result, &ctx, &counters);
        } else {
            line("  (no parseable property streams)", .{});
        }
    }

    printSkipSummary(p, &counters);
}
