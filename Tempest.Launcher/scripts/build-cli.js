import { execSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const dirname = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(dirname, "..");
const binariesDir = path.join(root, "electron", "resources", "binaries");

fs.mkdirSync(binariesDir, { recursive: true });

execSync(`dotnet publish ../Tempest.CLI -c Release -o ${binariesDir}`, {
	cwd: root,
	stdio: "inherit",
});

const files = fs.readdirSync(binariesDir);
for (const f of files) {
	const lower = f.toLowerCase();
	if (lower === "tempest.cli" || lower === "tempest.cli.exe") {
		const ext = lower.endsWith(".exe") ? ".exe" : "";
		fs.renameSync(path.join(binariesDir, f), path.join(binariesDir, `tempest-cli${ext}`));
	}
}

execSync("zig build --build-file ../Tempest.Utils/build.zig -Doptimize=ReleaseSafe", {
	cwd: root,
	stdio: "inherit",
});

const zigOut = path.join(root, "..", "Tempest.Utils", "zig-out", "bin");
if (fs.existsSync(zigOut)) {
	for (const f of fs.readdirSync(zigOut)) {
		if (f.startsWith("asmloader")) {
			fs.copyFileSync(path.join(zigOut, f), path.join(binariesDir, f));
		}
	}
}
