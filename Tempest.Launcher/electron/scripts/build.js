import { cpSync, rmSync } from "node:fs";
import { resolve } from "node:path";
import $ from "dax";

const root = resolve(import.meta.dirname, "../..");
const electronRoot = resolve(import.meta.dirname, "..");
await $`node scripts/build-cli.js`.cwd(root);
rmSync(resolve(electronRoot, "dist"), { recursive: true, force: true });
// Not `pnpm exec`: dax can't spawn .cmd shims under paths with spaces.
await $`tsc -p tsconfig.json`.cwd(electronRoot);
await $`vite build`.cwd(root);
rmSync(resolve(electronRoot, "build"), { recursive: true, force: true });
cpSync(resolve(root, "build"), resolve(electronRoot, "build"), { recursive: true });
await $`electron-builder`;
