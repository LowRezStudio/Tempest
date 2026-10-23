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

await $`pnpm exec tsc -p tsconfig.json`.cwd(electronRoot);
await $`vite build`.cwd(root);
rmSync(resolve(electronRoot, "build"), { recursive: true, force: true });
cpSync(resolve(root, "build"), resolve(electronRoot, "build"), { recursive: true });
await $`electron-builder`.cwd(electronRoot);
