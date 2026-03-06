export { createCommand, processArgs } from "./command";
export type { ArgumentType } from "./command";
export { getVersion, identifyBuild } from "./build";
export type { BuildInfo } from "./build";
export { launchGame, killGame } from "./launch";
export {
	getFieldsTokenPath,
	getFunctionsTokenPath,
	getInstanceAssemblyDbPath,
	getInstanceAssemblyDbConnectionString,
	getInstanceBasePath,
	getInstanceTokensDir,
} from "./paths";
