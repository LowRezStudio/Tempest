-- Force Vendors: the client's UI asks the online shop service for each vendor
-- (the loot tables GET_VENDOR sent at login) and only falls back to
-- assembly.dat when the service has none. Two UI functions are hooked so the
-- chosen vendors always take the fallback:
--   UiServerVendor(UIData_Vendor*)       -> shop vendor or 0 (sub_1414e0fb0 in 8.1)
--   UiEntryActive(UIData_LootTableItem*) -> is the entry active (sub_1414e7920)
-- Nothing on the purchase path calls either; the server still decides what
-- can be bought and owned.

local cfg = tempest.config({ vendors = {}, show_inactive = false })

local SIG_SERVER_VENDOR = "48 83 EC 28 48 8B D1 48 8B 0D ? ? ? ? 48 C7 44 24 38 00 00 00 00 48 85 C9 74 ? "
	.. "48 8B 01 8B 52 68 4C 8D 44 24 38 FF 50 78 48 8B 44 24 38 48 83 C4 28 C3"
local SIG_ENTRY_ACTIVE = "48 89 5C 24 18 57 48 83 EC 20 48 8B 51 6C 33 FF 48 8B D9 48 89 7C 24 30 48 85 D2 74 ? "
	.. "48 8B 0D ? ? ? ? 48 89 7C 24 38 48 85 C9 74 ? 48 8B 01 8B 52 68 4C 8D 44 24 38 FF 50 78 "
	.. "48 8B 4C 24 38 48 85 C9 74 ? 48 8B 01 8B 53 68 4C 8D 44 24 30 FF 50 18"

-- UIData_Vendor.m_dwVendorId, UIData_LootTableItem.m_dwLootId / m_pParentVendor
local VENDOR_ID, LOOT_ID, PARENT_VENDOR = 0x68, 0x68, 0x6C
-- CAmLootTableItem's active flag (what UiEntryActive returns offline)
local ITEM_ACTIVE = 0xF0

local function log(fmt, ...)
	print("[force-vendors] " .. string.format(fmt, ...))
end

local function find(name, sig)
	local hits = memory.scan(sig, 2)
	if #hits ~= 1 then
		log("%s: %d matches for its signature, not hooking (only the 8.1 Win64 client is known)", name, #hits)
		return nil
	end
	return hits[1]
end

local function rel32(at, len)
	return at + len + memory.read_i32(at + len - 4)
end

if tempest.status().arch == "x86" then
	log("the Win32 client is not supported")
	return
end

local server_vendor = find("UiServerVendor", SIG_SERVER_VENDOR)
local entry_active = find("UiEntryActive", SIG_ENTRY_ACTIVE)
if not server_vendor or not entry_active then
	return
end

-- UiEntryActive's assembly.dat fallback: lea rcx,[cache]; call LootTable; ...; call LootItem;
-- ...; movzx eax, byte [rax+0xF0]. Read the three targets from it, checking the opcodes.
local tail = entry_active + 0x99
if memory.read_u16(tail) ~= 0x8D48 or memory.read_u8(tail + 2) ~= 0x0D
	or memory.read_u8(entry_active + 0xA0) ~= 0xE8 or memory.read_u8(entry_active + 0xB0) ~= 0xE8
	or memory.read_u32(entry_active + 0xBA) ~= 0xF080B60F then
	log("UiEntryActive's fallback does not look as expected, not hooking")
	return
end
local AM_CACHE = rel32(tail, 7)
local AM_LOOT_TABLE = rel32(entry_active + 0xA0, 5)
local AM_LOOT_ITEM = rel32(entry_active + 0xB0, 5)

local forced = {}
for _, id in ipairs(cfg.vendors or {}) do
	forced[tonumber(id)] = true
end
local all = next(forced) == nil

local function is_forced(vendor)
	local id = vendor and vendor ~= 0 and memory.read_u32(vendor + VENDOR_ID)
	return id and (all or forced[id]) and id or nil
end

local stats = { vendors = 0, entries = 0, missing = 0 }

memory.hook(server_vendor, function(original, vendor)
	if is_forced(vendor) then
		stats.vendors = stats.vendors + 1
		return 0
	end
end, { args = 1 })

memory.hook(entry_active, function(original, entry)
	local vid = is_forced(memory.read_ptr(entry + PARENT_VENDOR))
	if not vid then
		return
	end
	stats.entries = stats.entries + 1
	local table_ = memory.call(AM_LOOT_TABLE, AM_CACHE, vid)
	local item = table_ ~= 0 and memory.call(AM_LOOT_ITEM, table_, memory.read_u32(entry + LOOT_ID)) or 0
	if item == 0 then
		stats.missing = stats.missing + 1
		return 0
	end
	if cfg.show_inactive then
		return 1
	end
	return memory.read_u8(item + ITEM_ACTIVE) or 0
end, { args = 1 })

log("hooked UiServerVendor at 0x%x, UiEntryActive at 0x%x (%s%s)", server_vendor, entry_active,
	all and "every vendor" or "vendors " .. table.concat(cfg.vendors, ", "),
	cfg.show_inactive and ", inactive entries shown" or "")

tempest.command("force_vendors", function(pc)
	pc:ClientMessage(string.format("force-vendors: %d vendor lookups, %d entry checks, %d entries not in assembly.dat",
		stats.vendors, stats.entries, stats.missing))
end, "force-vendors hook counters")
