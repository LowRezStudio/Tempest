import { resolve } from "node:path";
import $ from "dax";
import electron from "electron";

const root = resolve(import.meta.dirname, "../..");
const electronRoot = resolve(import.meta.dirname, "..");

// Compile the TypeScript host (main + preload) before starting it.
// Not `pnpm exec`: dax can't spawn .cmd shims under paths with spaces.
await $`tsc -p tsconfig.json`.cwd(electronRoot);

// No .bin shims: killing a shim's cmd.exe on Windows leaves vite running.
const vite = $`node ${resolve(root, "node_modules/vite/bin/vite.js")} dev`
	.cwd(root)
	.noThrow()
	.spawn();
const url = "http://localhost:1420";
for (let i = 0; i < 30; i++) {
	try {
		await fetch(url);
		break;
	} catch {
		await new Promise((r) => setTimeout(r, 1000));
	}
}
const app = $`${electron} .`.cwd(electronRoot).noThrow().spawn();

const [result, other] = await Promise.race([
	vite.then((r) => [r, app]),
	app.then((r) => [r, vite]),
]);
other.kill();
process.exit(result.code);
