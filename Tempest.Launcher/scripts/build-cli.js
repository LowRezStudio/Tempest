import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import $ from "dax";

const dirname = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(dirname, "..");
const repo = path.join(root, "..");
const binariesDir = path.join(root, "electron", "resources", "binaries");

fs.rmSync(binariesDir, { recursive: true, force: true });
fs.mkdirSync(binariesDir, { recursive: true });

// The CLI checks mods' "tempest" version requirements against the launcher version it ships with.
const version = JSON.parse(fs.readFileSync(path.join(root, "package.json"), "utf8")).version;

// The two pieces do not depend on each other and each is mostly one long
// compile job, so they run at the same time (a few cores is enough to keep
// them busy): the build takes as long as the slowest of them instead of the
// sum.
await Promise.all([
	// AOT publish, and the zig inject DLLs it embeds (BuildZigInject).
	$`dotnet publish ../Tempest.CLI -c Release -o ${binariesDir} -p:TempestVersion=${version}`.cwd(
		root,
	),
	// The upk tool applies mod deltas (deltas/<game path>.tdelta package entries)
	// for tempest-cli, which looks for it next to its own executable, and
	// LZO-compresses packages for the launcher's cooker.
	(async () => {
		await $`cargo build --release -p tempest-assets`.cwd(
			path.join(repo, "Tempest.UnrealScript"),
		);
		const upkName = process.platform === "win32" ? "upk.exe" : "upk";
		fs.copyFileSync(
			path.join(repo, "Tempest.UnrealScript", "target", "release", upkName),
			path.join(binariesDir, upkName),
		);
	})(),
]);

// Rename the AOT output and drop debug symbols: the sidecar ships without them.
for (const f of fs.readdirSync(binariesDir)) {
	const lower = f.toLowerCase();
	if (lower.endsWith(".dbg") || lower.endsWith(".pdb") || lower.endsWith(".dSYM")) {
		fs.rmSync(path.join(binariesDir, f), { recursive: true });
		continue;
	}
	if (lower === "tempest.cli" || lower === "tempest.cli.exe") {
		const ext = lower.endsWith(".exe") ? ".exe" : "";
		fs.renameSync(path.join(binariesDir, f), path.join(binariesDir, `tempest-cli${ext}`));
	}
}
