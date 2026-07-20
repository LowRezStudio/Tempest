const memory = @import("root").memory;

const Module = memory.Module;

// to-do: this is the x32 pattern, get the one for x64
pub fn getStaticFindObject(module: *Module) !*const void {
    const address = try module
        .scanner()
        .patternOnce("6A ? 68 ? ? ? ? 64 A1 ? ? ? ? 50 83 EC ? 53 55 56 57 A1 ? ? ? ? 33 C4 50 8D 44 24 ? 64 A3 ? ? ? ? 83 3D ? ? ? ? 00 75 ? 83 3D ? ? ? ? 00");

    return address.AsPtr(void);
}
