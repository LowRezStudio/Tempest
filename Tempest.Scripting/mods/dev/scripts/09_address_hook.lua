-- Step 9: hooks on raw addresses (C++ code, not UFunctions) and raw calls.
--   memory.hook(addr, fn, opts?) -> id   fn(original, a1, ..., aN) runs in place of the
--                                         function. Call original(...) to run it (nil or
--                                         missing args keep the caller's) and return its
--                                         result, return your own integer, or return
--                                         nothing to pass the call through untouched.
--   memory.unhook(id)                    opts.args = N (default 4, max 8)
--   memory.call(addr, ...)               call any address; integer/Object args, integer result
--   memory.alloc(n) / memory.free(p)     scratch buffers for pointer arguments
--   memory.write_str(p, s) / memory.write_wstr(p, s)   NUL-terminated ANSI / UTF-16
--   tempest.object(ptr)                  wrap a UObject* argument as an Object
-- Targets must be function entry points (the detour relocates the first
-- instructions). Only the Win64 integer registers and stack slots are seen:
-- RCX, RDX, R8, R9, then the stack; float arguments (XMM) are not. Handlers
-- run on whatever thread called the function, so keep them short and avoid
-- hooking render/audio/loading-thread code unless you mean to.
--
-- tempest_memhook <Pkg.Class.Func> [n]: trace the next n calls into a native
-- UFunction's C++ implementation (UFunction.Func) and show the `this` object:
--   tempest_memhook Engine.PlayerController.ConsoleCommand
-- A native already hooked with tempest.before/on runs through the runtime's
-- thunk instead; pick another one to watch the C++ entry itself.
tempest.command("tempest_memhook", function(pc, rest, argv)
	local spec = argv[1]
	if not spec then
		pc:ClientMessage("tempest_memhook <Pkg.Class.Func> [n]")
		return
	end
	local fn = tempest.find_object("Function " .. spec)
	if not fn then
		pc:ClientMessage("no function " .. spec)
		return
	end
	local addr = memory.read_ptr(fn:ptr() + 0xF0) -- UFunction.Func (8.1 layout, see unreal.rs)
	if not addr or addr == 0 or addr == tempest.status().process_internal then
		pc:ClientMessage(spec .. " is not native")
		return
	end
	local n = tonumber(argv[2]) or 3
	local id
	id = memory.hook(addr, function(original, this, frame, result)
		print(string.format("%s on %s (frame 0x%x, result 0x%x)", spec, tostring(tempest.object(this)), frame, result))
		n = n - 1
		if n <= 0 then
			tempest.queue(function() memory.unhook(id) end)
		end
		return original()
	end, { args = 3 })
	pc:ClientMessage(string.format("hooked %s at 0x%x for %d call(s)", spec, addr, n))
end, "tempest_memhook <Pkg.Class.Func> [n]  (trace a native's C++ entry)")

-- tempest_memhooks: what memory.hook currently intercepts.
tempest.command("tempest_memhooks", function(pc)
	local hooks = memory.hooks()
	for _, h in ipairs(hooks) do
		pc:ClientMessage(string.format("#%d 0x%x [%s]", h.id, h.addr, h.mod))
	end
	if #hooks == 0 then
		pc:ClientMessage("no address hooks")
	end
end, "list address hooks")
