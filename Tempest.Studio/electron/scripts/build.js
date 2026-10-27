// Production build: the Rust `upk` binary is built in release mode and
// bundled as a sidecar, the renderer is built by vite, the host by tsc.
import { cpSync, existsSync, mkdirSync, rmSync } from "node:fs";
import { resolve } from "node:path";
import $ from "dax";

const root = resolve(import.meta.dirname, "../..");
const electronRoot = resolve(import.meta.dirname, "..");
const rust = resolve(root, "..", "Tempest.UnrealScript");
const binaries = resolve(electronRoot, "resources", "binaries");

await $`cargo build --release -p tempest-assets`.cwd(rust);
mkdirSync(binaries, { recursive: true });
const exe = process.platform === "win32" ? "upk.exe" : "upk";
const built = resolve(rust, "target", "release", exe);
if (!existsSync(built)) throw new Error(`missing ${built}`);
cpSync(built, resolve(binaries, exe));

// tempest-cli for the mod cooker (pack, sign, install, publish), as the launcher bundles it.
const cliOut = resolve(binaries, "cli-publish");
await $`dotnet publish ${resolve(root, "..", "Tempest.CLI")} -c Release -o ${cliOut}`.cwd(root);
const cliExe = process.platform === "win32" ? "Tempest.CLI.exe" : "Tempest.CLI";
cpSync(
	resolve(cliOut, cliExe),
	resolve(binaries, process.platform === "win32" ? "tempest-cli.exe" : "tempest-cli"),
);
for (const lib of [
	"libblake3_dotnet.dylib",
	"libe_sqlite3.dylib",
	"libblake3_dotnet.so",
	"libe_sqlite3.so",
	"blake3_dotnet.dll",
	"e_sqlite3.dll",
]) {
	if (existsSync(resolve(cliOut, lib))) cpSync(resolve(cliOut, lib), resolve(binaries, lib));
}
rmSync(cliOut, { recursive: true, force: true });

await $`pnpm exec tsc -p tsconfig.json`.cwd(electronRoot);
await $`vite build`.cwd(root);
rmSync(resolve(electronRoot, "build"), { recursive: true, force: true });
cpSync(resolve(root, "build"), resolve(electronRoot, "build"), { recursive: true });
await $`electron-builder`.cwd(electronRoot);
