import { spawnSync } from "node:child_process";
import { glob, mkdir, rm } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const outDir = join(scriptDir, "../src/lib/rpc");

await rm(outDir, { recursive: true, force: true });
await mkdir(outDir, { recursive: true });

const protoDir = join(scriptDir, "../../Tempest.Protocol");
const protoFiles = await Array.fromAsync(
	glob("**/*.proto", {
		cwd: protoDir,
	}),
);

if (protoFiles.length === 0) {
	console.error("No proto files found!");
	process.exit(1);
}

const protocScript = join(scriptDir, "../node_modules/@protobuf-ts/protoc/protoc.js");
console.log("Generating code from .proto files");
const protoc = spawnSync(
	process.execPath,
	[
		protocScript,
		`--ts_out=${outDir}`,
		"--ts_opt",
		"server_none",
		`--proto_path=${protoDir}`,
		...protoFiles.map((f) => f.replaceAll("\\", "/")),
	],
	{ encoding: "utf8", stdio: "pipe" },
);
if (protoc.error) {
	console.error("Failed to run protoc:", protoc.error.message);
	process.exit(1);
}
if (protoc.status !== 0) {
	console.error(protoc.stderr || protoc.stdout);
	process.exit(protoc.status ?? 1);
}

console.log("Running oxfmt on the generated files");
const generatedFiles = await Array.fromAsync(glob("**/*.ts", { cwd: outDir })).then((files) =>
	files.map((f) => join(outDir, f)),
);
const oxfmt = spawnSync(
	process.execPath,
	[join(scriptDir, "../node_modules/oxfmt/bin/oxfmt"), "--write", ...generatedFiles],
	{ encoding: "utf8", stdio: "pipe" },
);
if (oxfmt.error) {
	console.error("Failed to run oxfmt:", oxfmt.error.message);
	process.exit(1);
}
if (oxfmt.status !== 0) {
	console.error(oxfmt.stderr || oxfmt.stdout);
	process.exit(oxfmt.status ?? 1);
}
