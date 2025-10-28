local ffi = require("ffi")

ffi.cdef[[
void op_console_print(const char* bytes);
]]

function print(...)
    local n = select("#", ...)

    if n == 0 then
        ffi.C.op_console_print("\n")
        return
    end

    if n == 1 then
        -- Fast path for single argument
        local v = ...
        local s = v == nil and "nil\n" or tostring(v) .. "\n"
        ffi.C.op_console_print(s)
        return
    end

    -- Multi-argument path
    local parts = {}
    for i = 1, n do
        local v = select(i, ...)
        parts[i] = v == nil and "nil" or tostring(v)
    end

    ffi.C.op_console_print(table.concat(parts, "\t") .. "\n")
end