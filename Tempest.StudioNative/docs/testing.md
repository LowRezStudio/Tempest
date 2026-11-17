# Native Studio: testing

[Agent entry point](../AGENTS.md). Read only the sections relevant to the task.

Paths in this reference retain their original project/repository context; verify details against
current sources.

## MCP server

**MCP server** (`src/mcp.rs`, `studio/mcp_tools.rs`): Streamable HTTP with JSON responses on
`127.0.0.1:1423/mcp` (`TEMPEST_STUDIO_MCP_PORT`, first window only), bearer token per run written
with the URL to `<config>/tempest-studio-native/mcp.json` and shown in Settings with the `claude mcp
add` command; tools `studio_state`, `studio_screenshot` (`src/screenshot.rs`: the OS takes the
window, `screencapture -l` on macOS, `PrintWindow` on Windows, none on Linux yet; PNG scaled to
`maxWidth`, optional `out`), `upk_call` (straight to the backend, binary payloads as base64),
`studio_open_workspace`, `studio_open_package`, `studio_select`, `studio_set_mode`,
`studio_commands`, `studio_run_command`, `studio_open_table`, `studio_close_dialog`,
`studio_project`, `studio_mod` (status/refresh/build), `studio_map` ([map reference](maps.md)); UI
tools run on the UI thread through a channel and answer once no work is in flight.

## Test driver

**Test driver** (`src/driver.rs`, `scripts/drive.py <steps> <out dir> [game dir|.twrk]`): with
`TEMPEST_DRIVE=1` the app reads steps from stdin, runs each, waits until no work is in flight,
prints `ok`; steps are input events (`key cmd-k`, `type …`, `click x y [right|double|cmd…]`, `click
@probe` on elements marked with `driver::probe(name)` (probes and input events share the window's
coordinates: the content runs under the transparent title bar), `@probe:x,y` for a point that far
from a probe's top-left corner (clicks, moves and both ends of `drag`), `drag`, `scroll x y dy
[dx]`, `size w h`, `wait ms`) or app steps (`package <name>`, `select <index|name>`, `run <command
id>`, `view home|champions|packages|assembly|audio|map|mod`, `tab props|refs|source`, `palette <prefix>`, `query
<text>`, `pick <row>`, `set <prop>=<value>`, `meshwire`, `meshlod <n>`, `material own-all|own
<slot>|scalar <name> <v>|pick-texture <param>|game on|off`, `sound select a b|edit|trim|fade-in s|gain
dB|play|apply`, `anim play|seek s`, `assembly …` (the steps are listed in [Assembly](assembly.md)); probes `game-<n>` (welcome), `task-*` (start page), `champ-<name>`, `skin-<n>`, `dev-<n>` and `file-<n>` (Champions), `cook-build`, `cooker refresh|preview <n>|name <text>|tab files|details|publish`,
`twrk-save <path>`, `twrk-open <path>`, `project new <folder> <name>|open <file>|close|save`,
`assembly skins|export-mod`, `wizard open <kind>|next|back|file <path>|query <text>|pick <n>|name
<text>|close`, `audio <pck name>`, `anim notify-move <k> <s>|notify-copy <k>|notify-remove
<k>|retime <s>`, `assembly mark <i>`, `meshgrid`, `meshskel`, `dialog close`); `shot <name>` lines
are taken by the script with `screencapture -l` on the app's window only (`scripts/winid.swift`).
Never save into the game folder from a test run.

`studio_state.assembly` is the Assembly view's `state_json` (file, path, table, row, column, total,
marks, columns, what is edited, unsaved files, undo labels). `studio_state.material` says whether the
material preview is drawn with game shaders (pair, animated, missing textures, or `gameError`);
`studio_state.meshGame` counts the mesh sections drawn with them and why the others are not. The wizard has `wizard range <a> <b>`
and `wizard dump` (step, a skin's parts, slots, next ids). A sweep is the cheapest crash hunt: open
every map (`map open`, collision, nav mesh, Kismet) and select objects of every class of many
packages through MCP (`studio_open_package`, `studio_select`), in a debug build (overflow checks) and
a release build (wgpu validation differs).
