import { resolve } from "node:path";
import $ from "dax";

const root = resolve(import.meta.dirname, "../..");
await $`node scripts/build-cli.js`.cwd(root);
await $`ELECTRON=true vite build`.cwd(root);
await $`electron-builder`;
