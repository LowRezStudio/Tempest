import { Command } from "@tauri-apps/plugin-shell";
import { assertNoGameSessions } from "./session-guards";
import type { SpawnOptions } from "@tauri-apps/plugin-shell";

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

const createDevCommand = (args: ArgumentType[], env?: SpawnOptions["env"]) =>
	Command.create(
		"dotnet",
		["exec", "../../Tempest.CLI/bin/Debug/net10.0/Tempest.CLI.dll", ...processArgs(args)],
		{ env },
	);

const createProdCommand = (args: ArgumentType[], env?: SpawnOptions["env"]) =>
	Command.sidecar("binaries/tempest-cli", processArgs(args), { env });

const createRuntimeCommand = import.meta.env.DEV ? createDevCommand : createProdCommand;

export const createCommand = (args: ArgumentType[], env?: SpawnOptions["env"]) => {
	const argumentsList = processArgs(args);
	if (
		argumentsList[0] === "mod" &&
		["install", "remove", "rename", "enable", "disable"].includes(argumentsList[1]) &&
		argumentsList[2]
	) {
		assertNoGameSessions(argumentsList[2]);
	}
	return createRuntimeCommand(args, env);
};
