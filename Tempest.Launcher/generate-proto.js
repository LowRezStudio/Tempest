import { glob, mkdir, rm } from "fs/promises";
import { join } from "path";
import $ from "dax";

const outDir = "src/lib/rpc";

await rm(outDir, { recursive: true, force: true });
await mkdir(outDir, { recursive: true });

const protoFiles = [];
for await (const f of glob("**/*.proto", {
	cwd: join(import.meta.dirname, "../Tempest.Protocol"),
})) {
	protoFiles.push(f);
}
const protoFilesStr = protoFiles.join(" ");
if (!protoFilesStr) {
	console.error("No proto files found!");
	process.exit(1);
}

console.log("Generating code from .proto files");
await $`pnpm exec protoc --ts_out ${outDir} --ts_opt server_none --proto_path ../Tempest.Protocol ${$.rawArg(protoFilesStr)}`;

console.log("Running prettier on the generated files");
await $`pnpm exec prettier --write ${outDir}/**/*.ts`;
