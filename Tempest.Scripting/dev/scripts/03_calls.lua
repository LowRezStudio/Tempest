-- Step 4: calling UFunctions (script and native) and inspecting them.
-- Runs once, on the game thread, the first time the HUD renders.
local done = false
tempest.on("PostRender", function(this, params)
    local pc = tempest.local_player()
    if done or pc == nil then return end
    done = true

    -- Introspection: a bound function knows its params and whether it is native.
    local fn = pc.GetHumanReadableName
    if fn then
        print(tostring(fn) .. " native=" .. tostring(fn:native()))
        for _, p in ipairs(fn:params()) do
            print(string.format("  %s %s%s%s", p.kind, p.name, p.out and " (out)" or "", p.ret and " (return)" or ""))
        end
        -- Return values come back as results.
        print("GetHumanReadableName() = " .. tostring(pc:GetHumanReadableName()))
    end

    -- Native call with an argument; both forms are equivalent.
    pc:ClientMessage("hello from 03_calls.lua")
    pc:call("ClientMessage", "and again via :call()")

    -- Anything typed in the console also runs here, on the game thread:
    --   =tempest.local_player():GetHumanReadableName()
    --   tempest.local_player():ConsoleCommand("stat fps")
    print("on game thread: " .. tostring(tempest.on_game_thread()))
end)
