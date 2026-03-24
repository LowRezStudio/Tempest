const memory = @import("root").memory;
const unreal = @import("root").unreal;

const Module = memory.Module;

export fn op_unreal_finders_StaticFindObject(module: *Module) *const void {
    return unreal.finders.getStaticFindObject(module);
}
