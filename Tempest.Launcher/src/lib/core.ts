import { Command } from "@tauri-apps/plugin-shell";
import type { Process } from "./types/process";
import type { Instance } from "./types/instance";
import { processesList } from "./stores/processes";
import { lastLaunchedInstanceId } from "./stores/instance";

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

export const getVersion = () =>
	createCommand(["--version"])
		.execute()
		.then((res) => res.stdout);

export type BuildInfo = {
	Id: string;
	VersionGroup: string;
	PatchHotfix: boolean;
	PatchName: string;
	PatchWikiReference: string;
};

export const identifyBuild = async (path: string): Promise<BuildInfo | null> => {
	const res = await createCommand(["build", "identify", path, "--json"]).execute();
	if (res.code !== 0) return null;
	try {
		return JSON.parse(res.stdout);
	} catch (error) {
		console.error("Failed to parse build info:", error);
		return null;
	}
};

export const launchGame = async (instance: Instance) => {
	const { path, launchOptions: options } = instance;

	lastLaunchedInstanceId.set(instance.id);

	const command = createCommand([
		"launch",
		path,
		{ "--no-default-args": options.noDefaultArgs },
		...(options.dllList ? options.dllList.map((dll) => ({ "--dll": dll })) : []),
		...(options.args ? ["--", ...processArgs(options.args)] : []),
	]);

	command.on("close", () => {
		processesList.set(processesList.get().filter((p) => p.instance.id !== instance.id));
	});

	const child = await command.spawn();

	const process: Process = {
		status: "on",
		child,
		command,
		instance,
	};

	processesList.set([...processesList.get(), process]);
};

export const killGame = async (instance: Instance) => {
	const processes = processesList.get();
	const process = processes.find((p) => p.instance.id === instance.id);

	if (process) await process.child.write("kill\n");
};
