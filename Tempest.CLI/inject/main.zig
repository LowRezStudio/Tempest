const std = @import("std");
const windows = std.os.windows;

const PROCESS_CREATE_THREAD = 0x0002;
const PROCESS_QUERY_INFORMATION = 0x0400;
const PROCESS_VM_OPERATION = 0x0008;
const PROCESS_VM_WRITE = 0x0020;
const PROCESS_VM_READ = 0x0010;

const MEM_COMMIT = 0x1000;
const MEM_RESERVE = 0x2000;
const PAGE_READWRITE = 0x04;
const MEM_RELEASE = 0x8000;
const WAIT_OBJECT_0 = 0x00000000;
const WAIT_ABANDONED = 0x00000080;
const WAIT_TIMEOUT = 0x00000102;
const WAIT_FAILED = 0xFFFFFFFF;
const STILL_ACTIVE = 259;

extern "kernel32" fn OpenProcess(dwDesiredAccess: windows.DWORD, bInheritHandle: windows.BOOL, dwProcessId: windows.DWORD) callconv(.winapi) ?windows.HANDLE;
extern "kernel32" fn VirtualAllocEx(hProcess: windows.HANDLE, lpAddress: ?*anyopaque, dwSize: windows.SIZE_T, flAllocationType: windows.DWORD, flProtect: windows.DWORD) callconv(.winapi) ?*anyopaque;
extern "kernel32" fn WriteProcessMemory(hProcess: windows.HANDLE, lpBaseAddress: *anyopaque, lpBuffer: *const anyopaque, nSize: windows.SIZE_T, lpNumberOfBytesWritten: ?*windows.SIZE_T) callconv(.winapi) windows.BOOL;
extern "kernel32" fn GetProcAddress(hModule: windows.HMODULE, lpProcName: [*:0]const u8) callconv(.winapi) ?*anyopaque;
extern "kernel32" fn GetModuleHandleA(lpModuleName: ?[*:0]const u8) callconv(.winapi) ?windows.HMODULE;
extern "kernel32" fn CreateRemoteThread(hProcess: windows.HANDLE, lpThreadAttributes: ?*anyopaque, dwStackSize: windows.SIZE_T, lpStartAddress: *anyopaque, lpParameter: ?*anyopaque, dwCreationFlags: windows.DWORD, lpThreadId: ?*windows.DWORD) callconv(.winapi) ?windows.HANDLE;
extern "kernel32" fn VirtualFreeEx(hProcess: windows.HANDLE, lpAddress: *anyopaque, dwSize: windows.SIZE_T, dwFreeType: windows.DWORD) callconv(.winapi) windows.BOOL;
extern "kernel32" fn GetLastError() callconv(.winapi) windows.DWORD;
extern "kernel32" fn WaitForSingleObject(hHandle: windows.HANDLE, dwMilliseconds: windows.DWORD) callconv(.winapi) windows.DWORD;
extern "kernel32" fn GetExitCodeThread(hThread: windows.HANDLE, lpExitCode: *windows.DWORD) callconv(.winapi) windows.BOOL;
extern "kernel32" fn CloseHandle(hObject: windows.HANDLE) callconv(.winapi) windows.BOOL;

pub fn main() !void {
    var arena = std.heap.ArenaAllocator.init(std.heap.page_allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    const args = try std.process.argsAlloc(allocator);
    if (args.len != 3) {
        std.debug.print("Usage: <pid> <dll_path>\n", .{});
        std.process.exit(1);
    }

    const pid = std.fmt.parseInt(u32, args[1], 10) catch {
        std.debug.print("Error: Invalid PID\n", .{});
        std.process.exit(1);
    };

    const dll_path = args[2];
    std.debug.print("Injecting {s} into process {d}\n", .{ dll_path, pid });

    const process_handle = OpenProcess(
        PROCESS_CREATE_THREAD | PROCESS_QUERY_INFORMATION | PROCESS_VM_OPERATION | PROCESS_VM_WRITE | PROCESS_VM_READ,
        0,
        pid,
    ) orelse {
        const error_code = GetLastError();
        std.debug.print("Error: Failed to open process with PID {d} (Error: {d})\n", .{ pid, error_code });
        std.process.exit(1);
    };

    const kernel32 = GetModuleHandleA("kernel32.dll") orelse {
        const error_code = GetLastError();
        std.debug.print("Error: Failed to get kernel32.dll handle (Error: {d})\n", .{error_code});
        _ = CloseHandle(process_handle);
        std.process.exit(1);
    };

    const loadlibrary_addr = GetProcAddress(kernel32, "LoadLibraryA") orelse {
        const error_code = GetLastError();
        std.debug.print("Error: Failed to get LoadLibraryA address (Error: {d})\n", .{error_code});
        _ = CloseHandle(process_handle);
        std.process.exit(1);
    };

    const dll_path_len = dll_path.len + 1;
    const remote_memory = VirtualAllocEx(
        process_handle,
        null,
        dll_path_len,
        MEM_COMMIT | MEM_RESERVE,
        PAGE_READWRITE,
    ) orelse {
        const error_code = GetLastError();
        std.debug.print("Error: Failed to allocate memory in target process (Error: {d})\n", .{error_code});
        _ = CloseHandle(process_handle);
        std.process.exit(1);
    };

    const dll_path_z = try allocator.dupeZ(u8, dll_path);
    if (WriteProcessMemory(process_handle, remote_memory, dll_path_z.ptr, dll_path_len, null) == 0) {
        const error_code = GetLastError();
        std.debug.print("Error: Failed to write DLL path to target process (Error: {d})\n", .{error_code});
        _ = VirtualFreeEx(process_handle, remote_memory, 0, MEM_RELEASE);
        _ = CloseHandle(process_handle);
        std.process.exit(1);
    }

    const thread_handle = CreateRemoteThread(
        process_handle,
        null,
        0,
        loadlibrary_addr,
        remote_memory,
        0,
        null,
    ) orelse {
        const error_code = GetLastError();
        std.debug.print("Error: Failed to create remote thread (Error: {d})\n", .{error_code});
        _ = VirtualFreeEx(process_handle, remote_memory, 0, MEM_RELEASE);
        _ = CloseHandle(process_handle);
        std.process.exit(1);
    };

    const wait_result = WaitForSingleObject(thread_handle, windows.INFINITE);

    if (wait_result != WAIT_OBJECT_0) {
        switch (wait_result) {
            WAIT_ABANDONED => std.debug.print("Error: Remote thread wait was abandoned\n", .{}),
            WAIT_TIMEOUT => std.debug.print("Error: Timed out waiting for remote thread\n", .{}),
            WAIT_FAILED => {
                const error_code = GetLastError();
                std.debug.print("Error: Wait for remote thread failed (Error: {d})\n", .{error_code});
            },
            else => std.debug.print("Error: Unexpected wait result {d}\n", .{wait_result}),
        }
        _ = VirtualFreeEx(process_handle, remote_memory, 0, MEM_RELEASE);
        _ = CloseHandle(thread_handle);
        _ = CloseHandle(process_handle);
        std.process.exit(1);
    }

    var exit_code: windows.DWORD = 0;
    if (GetExitCodeThread(thread_handle, &exit_code) == 0) {
        const error_code = GetLastError();
        std.debug.print("Error: Failed to get remote thread exit code (Error: {d})\n", .{error_code});
        _ = VirtualFreeEx(process_handle, remote_memory, 0, MEM_RELEASE);
        _ = CloseHandle(thread_handle);
        _ = CloseHandle(process_handle);
        std.process.exit(1);
    }

    if (exit_code == 0 or exit_code == STILL_ACTIVE) {
        std.debug.print("Error: Remote LoadLibraryA returned a null module handle\n", .{});
        _ = VirtualFreeEx(process_handle, remote_memory, 0, MEM_RELEASE);
        _ = CloseHandle(thread_handle);
        _ = CloseHandle(process_handle);
        std.process.exit(1);
    }

    if (VirtualFreeEx(process_handle, remote_memory, 0, MEM_RELEASE) == 0) {
        const error_code = GetLastError();
        std.debug.print("Warning: Failed to free remote memory (Error: {d})\n", .{error_code});
    }

    _ = CloseHandle(thread_handle);
    _ = CloseHandle(process_handle);

    std.debug.print("DLL injection completed successfully (module handle: 0x{x})\n", .{exit_code});
}
