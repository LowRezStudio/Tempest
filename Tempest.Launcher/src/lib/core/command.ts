import { Command } from "@tauri-apps/plugin-shell";

export type ArgumentType =
	| string
	| Record<string, string | boolean | undefined | null>
	| ArgumentType[]
	| boolean
	| undefined
	| null;

export const processArgs = (args: ArgumentType[]): string[] =>
	args.flatMap((arg) => {
		if (arg == null) return [];
		if (Array.isArray(arg)) return processArgs(arg);
		if (typeof arg === "object") {
			return Object.entries(arg)
				.filter(([, val]) => !!val)
				.flatMap(([key, val]) => (typeof val === "string" ? [key, val] : [key]));
		}
		if (typeof arg === "string") return [arg];

		return [];
	});

const createDevCommand = (args: ArgumentType[]) =>
	Command.create("dotnet", [
		"run",
		"--project",
		"../../Tempest.CLI/Tempest.CLI.csproj",
		"--property",
		"WarningLevel=0",
		"--",
		...processArgs(args),
	]);

const createProdCommand = (args: ArgumentType[]) =>
	Command.sidecar("binaries/tempest-cli", processArgs(args));

export const createCommand = import.meta.env.DEV ? createDevCommand : createProdCommand;
