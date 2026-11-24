import { spawnSync } from "node:child_process";
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

// Copies the outputs a zig build installed, failing when it produced none.
function installFrom(outDir, matches) {
	const copied = [];
	if (fs.existsSync(outDir)) {
		for (const f of fs.readdirSync(outDir)) {
			if (matches(f)) {
				fs.copyFileSync(path.join(outDir, f), path.join(binariesDir, f));
				copied.push(f);
			}
		}
	}
	if (copied.length === 0) throw new Error(`no sidecar binaries found in ${outDir}`);
	return copied;
}

// Zig <0.16 on macOS 26 needs the zign wrapper to link (same rule as the
// justfile's utils recipes); override with ZIG=.
const zig =
	process.env.ZIG ??
	(spawnSync("command", ["-v", "zign"], { shell: true }).status === 0 ? "zign" : "zig");

// faststart (Tempest.Utils): the startup-fix DLL the CLI injects into the 8.1
// Win64 client, cross-compiled whatever the host is.
const buildFastStart = async () => {
	const pkg = path.join(repo, "Tempest.Utils");
	await $`${zig} build -Doptimize=ReleaseSafe`.cwd(pkg);
	installFrom(
		path.join(pkg, "zig-out", "bin"),
		(f) => f.startsWith("faststart") && !f.toLowerCase().endsWith(".pdb"),
	);
};

// The three pieces do not depend on each other and each is mostly one long
// compile job, so they run at the same time (a few cores is enough to keep
// them busy): the build takes as long as the slowest of them instead of the
// sum.
await Promise.all([
	// AOT publish, with the zig injector it ships for Wine (BuildZigInject).
	$`dotnet publish ../Tempest.CLI -c Release -o ${binariesDir} -p:TempestVersion=${version}`.cwd(
		root,
	),
	// The upk tool applies mod deltas (deltas/<game path>.tdelta package entries)
	// for the CLI, which looks for it next to its own executable, and
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
	buildFastStart(),
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
		fs.renameSync(path.join(binariesDir, f), path.join(binariesDir, `tempest${ext}`));
	}
}
