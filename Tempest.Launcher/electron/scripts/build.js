import { cpSync, rmSync } from "node:fs";
import { resolve } from "node:path";
import $ from "dax";

const root = resolve(import.meta.dirname, "../..");
const electronRoot = resolve(import.meta.dirname, "..");
await $`node scripts/build-cli.js`.cwd(root);
await $`ELECTRON=true vite build`.cwd(root);
rmSync(resolve(electronRoot, "build"), { recursive: true, force: true });
cpSync(resolve(root, "build"), resolve(electronRoot, "build"), { recursive: true });
await $`electron-builder`;
