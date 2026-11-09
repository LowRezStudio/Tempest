# Studio testing and MCP

[Agent entry point](../AGENTS.md). Read only the sections relevant to the task.

Paths in this reference retain their original project/repository context; verify details against
current sources.

- Screenshot harness for checking the UI without a human: `STUDIO_SMOKE_SCRIPT=<file.js>
  STUDIO_SMOKE_OUT=<out.png> electron .` (dev build, vite running on 1421) runs the script in the
  renderer against `window.studio` (the store, exposed in dev builds), then writes a PNG of the
  window and exits (`electron/src/smoke.ts`). The dev server pre-bundles three.js and CodeMirror
  (`optimizeDeps`) so it never reloads mid-session; the session (workspace, package, selection) is
  restored from `sessionStorage` after a reload.

- **MCP server** (`electron/src/mcp.ts`, toolbar plug icon): Streamable HTTP on `127.0.0.1:1423/mcp`
  (`TEMPEST_STUDIO_MCP_PORT`), bearer token generated at start and written with the URL to
  `<userData>/mcp.json`; the dialog shows the `claude mcp add` command and the `.mcp.json` snippet.
  Tools: `studio_windows`, `studio_state` (view, project and its save state, edited files, open
  dialog, pending confirmation, tabs, history, cooker summary), `upk_call` (any backend method of a
  window, binary payloads as base64), `studio_open_workspace`, `studio_open_package`,
  `studio_select`, `studio_set_mode` (packages/assembly/map/mod), `studio_map` (list/open maps,
  levels, actors by category, select, move by location/rotation (`map.setTransform` numbers, one
  undo step), show/hide categories and levels, shading, a level's Kismet, link/unlink,
  palette/addNode/removeNode, heavyWalls/canBaseOn, actorPalette/addActor/removeActor, export,
  matinee/matinees/newMatinee/setKey/addKey/removeKey/setInterpMode/setTangents/keyAtPlayhead/addGroup/removeGroup/setGroup/addTrack/removeTrack/setProps/connect/matineeTime,
  collision/brush/setBrush, bake/lightInfo, navmesh, undo), `studio_open_table`, `studio_commands` /
  `studio_run_command` (the command registry), `studio_confirm` (answers `ui.confirm`; actions that
  ask return early with `status: "confirmation"` through the `watch` helper instead of blocking),
  `studio_close_dialog`, `studio_project` (status/create/open/save/close), `studio_mod` (cooker
  status/refresh/preview/set/build/install; publishing is left to the user), `studio_swf` (the movie
  editor, `src/lib/studio/swf-tool.ts` driving the same `SwfDoc` as the UI, so the window follows:
  list movies, go to a sprite/frame/character, character tree, timeline objects with
  x/y/scale/rotation/alpha, texts/scripts/images/linkage/tags, a script's decompiled source and
  P-code (`script`) and replacing it (`setScript`, compiled first), pending placement/text edits
  (also a raw colour transform `cxform`), a character's details (`character`: shape colours and
  widths with their keys, font glyphs, static text records, edit-text properties, sound format) and
  its edits (`shapeStyle`, `shapeSvg`, `editText` properties, `staticText`, `addGlyphs`), images
  (`addImage`, `replaceImage`, `exportImage`), `export` (SVG or PNG of a character, sprite frame or
  root frame, TrueType, sounds, binary data), `compile` (check ActionScript or P-code), render-only
  layer hiding, apply as one undo step, discard; `SwfDoc.settled()` waits until the views include
  the edits and re-reads the movie after an undo or discard), `studio_eval` (code run in the
  renderer with every store in scope: `studio`, `assembly`, `rpc`, `session`, `ui`, `tabs`,
  `commands`, `nav`, `cooker`, `map`, `collision`, `swfDoc`, `swfTool`, plus `watch`, through
  `window.__tempest`, set in `src/routes/+layout.svelte`), `studio_screenshot`, `studio_new_window`.
