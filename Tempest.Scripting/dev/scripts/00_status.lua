-- Step 1: did the DLL resolve the game? Everything else builds on this.
-- Expect gnames_ok=true, gobjects_ok=true, hook_installed=true and no notes.
local st = tempest.status()
local function hex(v) return v and string.format("0x%x", v) or "nil" end

print("Tempest.Scripting " .. tempest.version)
print("GObjects      " .. hex(st.gobjects) .. "  ok=" .. tostring(st.gobjects_ok) .. "  count=" .. tostring(st.object_count))
print("GNames        " .. hex(st.gnames) .. "  ok=" .. tostring(st.gnames_ok) .. "  count=" .. tostring(st.name_count))
print("GEngine slot  " .. hex(st.engine))
print("ProcessEvent  " .. hex(st.process_event) .. "  hooked=" .. tostring(st.hook_installed))
print("xor mask      " .. hex(st.xor_mask))
for _, note in ipairs(st.notes) do
    print("NOTE: " .. note)
end

-- A few live objects, to eyeball that names decode.
for i, obj in ipairs(tempest.objects(5)) do
    print(string.format("object[%d] = %s", i, tostring(obj)))
end

-- Class lookup walks GObjects once, then it is cached.
local class = tempest.find_class("Engine.PlayerController")
print("Engine.PlayerController class: " .. tostring(class))

-- nil in menus; the local controller once in a match.
print("local player: " .. tostring(tempest.local_player()))
