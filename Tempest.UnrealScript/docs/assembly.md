# UnrealScript: assembly

[Agent entry point](../AGENTS.md). Read only the sections relevant to the task.

Paths in this reference retain their original project/repository context; verify details against
current sources.

### MCTS marshals (`crates/tempest-assets/src/mcts/`)

`assembly.dat` (CookedPCConsole), `Localization/<EXT>/Lang_<EXT>.dat` and `WordFilter.dat` are
Hi-Rez `CMarshal` streams XOR'd with 0x2A (no compression, no hash): `u8 flags`, `u32 function`
(FNV-1 of the name; `GET_DATA_ASSEMBLY`, 0 for localization), then a row (`u16` entry count, 0xFFFF
→ `u32`) of entries whose `u16` header is kind (bits 15..12) and parameter (bits 5..0): 1..4 batches
of 8/16/32/64-bit numbers (`param` tokens then values), 5/9 legacy strings, 6 row set, 7 GUID, 8
blob, 10 string with the encoding in the parameter (the files use 4, UTF-16). Fields are tokens into
the game's `MFIELD_TOKEN` enum; `mod.rs` holds the reader, the writer (which follows
`CMarshalRow::StoreEntries`: per row the numbers are batched into four width caches flushed when 32
are gathered and at the row's end in width order, the width from the field's `MFIELD_TYPES` class
and, for ints/dwords/floats and words, from the value; 64-bit for double/qword/netid/datetime) and
`Tokens` (the field and function tables: extracted at runtime from the game's
`Binaries/Win64/MctsInterface.dll` next to the cooked folder, gzip blobs with `VERSION` / `HELLO`,
else the built-in copies from 8.1 in `data/mcts/`). `doc.rs` is an editable document (rows addressed
by a path of `(field, index)` pairs, undo journal, paged/filtered/sorted queries), `workspace.rs`
the assembly + languages of a cooked folder with the JSON shapes `upk serve` exposes as `mcts.*`
(open, table, rows, row, set, setHeader, insert, delete, undo, save, discard, find, messages,
setMessage, validValues). Correctness: `upk mcts verify <dir>` must rewrite every `.dat`
byte-identically (it does for 8.1 and 8.2). Verified against `CMarshalRow::LoadEntries` /
`StoreEntries` and `CPackage::ReadFromFile` in `libMctsInterface.so`. Unknown: whether the client
compares `GAME_VERSION_RAW` (four `u16`: 8.1.5839.0) with its own build; the editor keeps it and
lets it be edited. `behavior_trees.dat` is not a marshal.

### Content creation (`mcts/create.rs`)

Devices (abilities, weapons, cards and talents are all `DATA_SET_DEVICES` + `DATA_SET_ITEMS` rows
with one id), items and champions are cloned from a template row: ids past the current maxima,
nested `DEVICE_MODE_ID`s reallocated, messages created in every language. `scaffold` writes the mod
folder (see [Studio mod workflow](../../Tempest.Studio/docs/mods-and-workspaces.md)). A champion's
visibility in the client is decided by loot table 475 "Champion Vendor" (`DATA_SET_LOOT_TABLES` /
`DATA_SET_LOOT_TABLE_ITEMS`): the native `UIDataChampions` init (8.1 client `sub_1415b7d60`) walks
that table's entries, skips inactive ones (`ACTIVE_FLAG`), makes a `UIDataChampion` per Character
Class item (`m_nBotId` = the item's `REF_BOT_ID`) and fills `m_Champions` + `m_ChampionIdToData`; no
server data is involved, which is why an offline client still lists every champion.
`create_champion` therefore appends an entry for the new Character Class item (`add_vendor_entry`:
clone of the template's entry, fresh `LOOT_TABLE_ITEM_ID`, next `SORT_ORDER`, active, without the
template's own `*_MSG_ID` overrides); `mcts.vendor` reports the table (active/total, listed entries,
champion bots with a Character Class item but no entry) and `mcts.vendorAdd {bot}` adds one for an
existing bot. Skins (`mcts/skins.rs`): the client lists a champion's skins from loot table 1076
"Inventory Vendor - Collections" (8.1 client `sub_141595550` reads table 0x434 and groups the
entries per champion; `sub_1414c6770` resolves a collection item, type 0x662 = 1634, through its
entry's sub loot table to the item of type 0x27f6 = 10230, the skin). An entry's `ITEM_ID` is a
container item on the champion, `SUB_LOOT_TABLE_ID` a type-1455 table of the parts (skin, head
15170, weapon 15041, voice, entitlement 11403 whose prop 2143 is the entry's `LOOT_TABLE_ITEM_ID`
and `REF_ITEM_ID` a container of the set; some entries name it as `REQUIRED_ITEM_ID`). `create_skin`
copies all of it with fresh ids (`mcts.create {kind: "skin"}`), `collection_add` wraps existing
items in a new collection without an entitlement, `collections` lists them (indexed once: 20 ms for
784 entries). Default skins are in the champion's "SLT - Collection/Bundle - <name> - Default"
tables, not in 1076. The 8.2 server's `SpawnBotAllSkins` cheat reads table 476 instead, which 8.1
does not have. The in-match select movie has 60 button clips and the native binding loop is
hard-coded to 60 (8.1 ships 60 active entries), so a 61st champion needs one deactivated.
Playability also needs the private server to list the champion (`server_rows`).
