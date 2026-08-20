import { lastLaunchedInstanceId } from "../stores/instance.svelte";
import { appendProcessLog, logCommandOutput, processesList } from "../stores/processes.svelte";
import {
	gamescopeArgs,
	protonPath,
	useGamescope,
	useSteamRuntime,
	winePath,
	wineRuntime,
} from "../stores/settings.svelte";
import { createCommand, processArgs } from "./command";
import type { Instance } from "../types/instance";
import type { Process } from "../types/process";

export const launchGame = async (instance: Instance) => {
	const { path, launchOptions: options } = instance;
	const platform = options.platform ?? "Win64";

	lastLaunchedInstanceId.value = instance.id;

	console.log("Launching instance", instance.id, instance.version);
	console.log(instance);

	appendProcessLog(
		`Launching game client with args: ${processArgs(options.args ?? []).join(" ")}`,
		false,
		"launch",
	);

	const wine = winePath.get();
	const runtime = wineRuntime.get();
	const proton =
		runtime === "proton" ? protonPath.get() : runtime === "wine" ? "wine" : undefined;
	const env: Record<string, string> | undefined =
		wine || proton
			? { ...(wine ? { WINE: wine } : {}), ...(proton ? { PROTON: proton } : {}) }
			: undefined;

	const command = createCommand(
		[
			"launch",
			path,
			{ "--platform": platform },
			{ "--no-default-args": options.noDefaultArgs },
			{ "--gamescope": useGamescope.get() },
			{ "--gamescope-args": gamescopeArgs.get() || undefined },
			{ "--steam-runtime": useSteamRuntime.get() || undefined },
			{
				"--homedir":
					instance.version === "8.1"
						? "Paladins"
						: `${instance.version ? `${instance.version}_` : ""}${instance.label}`.replaceAll(
								/[^a-zA-Z0-9-_]/g,
								"_",
							),
			},
			...(options.dllList ? options.dllList.map((dll) => ({ "--dll": dll })) : []),
			...(options.args ? ["--", ...processArgs(options.args)] : []),
		],
		env,
	);

	logCommandOutput(command, "launch");

	command.on("close", () => {
		processesList.value = processesList.value.filter((p) => p.instance.id !== instance.id);
	});

	const child = await command.spawn();

	const process: Process = {
		status: "on",
		child,
		command,
		instance,
	};

	processesList.value = [...processesList.value, process];
};

export const killGame = async (instance: Instance) => {
	const processes = processesList.value;
	const process = processes.find((p: Process) => p.instance.id === instance.id);

	if (process) await process.child.write("kill\n");
};
