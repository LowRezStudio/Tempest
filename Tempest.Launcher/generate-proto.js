import { execSync } from "child_process";
import { existsSync, mkdirSync, rmSync } from "fs";
import { globSync } from "glob";

//A cross-platform script to generate code from the .proto definitions
const outDir = "src/lib/rpc";
if (existsSync(outDir)) {
	rmSync(outDir, { recursive: true, force: true });
}
mkdirSync(outDir);

const protoFiles = globSync("../Tempest.Protocol/**/*.proto").join(" ");
if (!protoFiles) {
	console.error("No proto files found!");
	process.exit(1);
}

const protocCmd = `protoc --ts_out ${outDir} --ts_opt server_none --proto_path ../Tempest.Protocol ${protoFiles}`;
const prettierCmd = `prettier --write "${outDir}/**/*.ts"`;

try {
	console.log("Generating code from .proto files");
	execSync(protocCmd, { stdio: "inherit" });
	console.log("Running prettier on the generated files");
	execSync(prettierCmd, { stdio: "inherit" });
} catch (error) {
	console.error("Generation failed:", error.message);
	process.exit(1);
}
