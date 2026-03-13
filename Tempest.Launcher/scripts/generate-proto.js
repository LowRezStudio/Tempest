import { glob, mkdir, rm } from "fs/promises";
import { join } from "path";
import $ from "dax";

const outDir = join(import.meta.dirname, "../src/lib/rpc");

await rm(outDir, { recursive: true, force: true });
await mkdir(outDir, { recursive: true });

const protoFiles = await Array.fromAsync(
	glob("**/*.proto", {
		cwd: join(import.meta.dirname, "../../Tempest.Protocol"),
	}),
);

const protoFilesStr = protoFiles.join(" ");
if (!protoFilesStr) {
	console.error("No proto files found!");
	process.exit(1);
}

console.log("Generating code from .proto files");
await $`protoc --ts_out ${outDir} --ts_opt server_none --proto_path ../Tempest.Protocol ${$.rawArg(protoFilesStr)}`;

console.log("Running prettier on the generated files");
await $`prettier --write ${outDir}/**/*.ts`;
