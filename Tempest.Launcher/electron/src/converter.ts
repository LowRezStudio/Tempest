// V1 -> V2 converter imports: the host shows the file dialog and reads the
// picked/dropped files in one round trip. Parsing and package assembly stay in
// the renderer; only raw bytes cross the bridge.
import fs from "node:fs";
import path from "node:path";
import { BrowserWindow, dialog } from "electron";
import { handle } from "./ipc/handle.js";
import type { ConverterImport } from "./ipc/contract.js";

const IMPORT_EXTENSIONS = new Set(["tempest", "upk", "pck", "ini", "dll", "lua"]);

async function readImports(paths: string[]): Promise<ConverterImport[]> {
	const imports: ConverterImport[] = [];
	for (const filePath of paths) {
		const ext = path.extname(filePath).toLowerCase().replace(/^\./, "");
		if (!IMPORT_EXTENSIONS.has(ext)) continue;
		const data = await fs.promises.readFile(filePath);
		imports.push({
			path: filePath,
			name: path.basename(filePath),
			data: data.buffer.slice(data.byteOffset, data.byteOffset + data.byteLength),
		});
	}
	return imports;
}

handle("converter:pick-imports", async (event) => {
	const win = BrowserWindow.fromWebContents(event.sender);
	const result = await dialog.showOpenDialog(win!, {
		properties: ["openFile", "multiSelections"],
		filters: [{ name: "Mod Files", extensions: [...IMPORT_EXTENSIONS] }],
	});
	if (result.canceled) return [];
	return readImports(result.filePaths);
});

handle("converter:read-imports", (_event, { paths }) => readImports(paths));

handle("converter:pick-icon", async (event) => {
	const win = BrowserWindow.fromWebContents(event.sender);
	const result = await dialog.showOpenDialog(win!, {
		properties: ["openFile"],
		filters: [{ name: "Image", extensions: ["png"] }],
	});
	const picked = result.filePaths[0];
	if (result.canceled || !picked) return null;
	const data = await fs.promises.readFile(picked);
	return {
		path: picked,
		name: path.basename(picked),
		data: data.buffer.slice(data.byteOffset, data.byteOffset + data.byteLength),
	};
});
