import { path } from "@tauri-apps/api";
import { resolveResource, tempDir } from "@tauri-apps/api/path";
import { mkdir, readFile, remove, writeFile } from "@tauri-apps/plugin-fs";
import { createCommand } from "$lib/core/command";
import { protonPath, winePath, wineRuntime } from "$lib/stores/settings.svelte";

export async function compressUpk(data: Uint8Array): Promise<Uint8Array> {
	const tool = await resolveResource("upk-parser.exe");
	const proton =
		wineRuntime.value === "proton"
			? protonPath.value
			: wineRuntime.value === "wine"
				? "wine"
				: undefined;
	const dir = await path.join(await tempDir(), `tempest-upk-${crypto.randomUUID()}`);
	await mkdir(dir);
	try {
		const input = await path.join(dir, "input.upk");
		const output = await path.join(dir, "output.upk");
		await writeFile(input, data);
		const result = await createCommand(
			["mod", "compress-upk", input, { "--output": output, "--tool": tool }],
			{
				...(winePath.value ? { WINE: winePath.value } : {}),
				...(proton ? { PROTON: proton } : {}),
			},
		).execute();
		if (result.code !== 0) throw new Error(result.stderr.trim() || "UPK compression failed.");
		return await readFile(output);
	} finally {
		await remove(dir, { recursive: true });
	}
}
