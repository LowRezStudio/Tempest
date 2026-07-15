import { resolve } from "node:path";
import $ from "dax";

const root = resolve(import.meta.dirname, "../..");
const vite = $`ELECTRON=true vite dev`.cwd(root).spawn();
const url = "http://localhost:1420";
for (let i = 0; i < 30; i++) {
	try {
		await fetch(url);
		break;
	} catch {
		await new Promise((r) => setTimeout(r, 1000));
	}
}
const electron = $`electron .`.spawn();

const last = await Promise.race([vite, electron]);
const other = last === vite ? electron : vite;
other.kill();
process.exit(last.exitCode ?? 0);
