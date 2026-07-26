import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import $ from "dax";

const dirname = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(dirname, "..");
const binariesDir = path.join(root, "electron", "resources", "binaries");

fs.mkdirSync(binariesDir, { recursive: true });

await $`dotnet publish ../Tempest.CLI -c Release -o ${binariesDir}`.cwd(root);

const files = fs.readdirSync(binariesDir);
for (const f of files) {
	const lower = f.toLowerCase();
	if (lower.endsWith(".dbg") || lower.endsWith(".pdb")) {
		fs.rmSync(path.join(binariesDir, f));
		continue;
	}
	if (lower === "tempest.cli" || lower === "tempest.cli.exe") {
		const ext = lower.endsWith(".exe") ? ".exe" : "";
		fs.renameSync(path.join(binariesDir, f), path.join(binariesDir, `tempest-cli${ext}`));
	}
}

await $`zig build -Doptimize=ReleaseSafe`.cwd(path.join(root, "..", "Tempest.Utils"));

const zigOut = path.join(root, "..", "Tempest.Utils", "zig-out", "bin");
if (fs.existsSync(zigOut)) {
	for (const f of fs.readdirSync(zigOut)) {
		if (f.startsWith("asmloader") && !f.toLowerCase().endsWith(".pdb")) {
			fs.copyFileSync(path.join(zigOut, f), path.join(binariesDir, f));
		}
	}
}
