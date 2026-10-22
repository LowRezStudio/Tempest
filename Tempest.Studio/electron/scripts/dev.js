import { resolve } from "node:path";
import $ from "dax";

const root = resolve(import.meta.dirname, "../..");
const electronRoot = resolve(import.meta.dirname, "..");

// The host finds the `upk` binary in the Rust workspace's release output in
// dev (see src/upk.ts); build it first so a fresh checkout works.
if (!process.env.STUDIO_SKIP_CARGO) {
	await $`cargo build --release -p tempest-assets`.cwd(
		resolve(root, "..", "Tempest.UnrealScript"),
	);
}
await $`pnpm exec tsc -p tsconfig.json`.cwd(electronRoot);

const vite = $`vite dev`.cwd(root).spawn();
const url = "http://localhost:1421";
for (let i = 0; i < 30; i++) {
	try {
		await fetch(url);
		break;
	} catch {
		await new Promise((r) => setTimeout(r, 1000));
	}
}
const electron = $`electron .`.cwd(electronRoot).spawn();

const last = await Promise.race([vite, electron]);
const other = last === vite ? electron : vite;
other.kill();
process.exit(last.exitCode ?? 0);
