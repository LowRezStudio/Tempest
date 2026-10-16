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
				"--homedir": (options.args ?? []).some((arg) => /^-homedir(?:=|$)/i.test(arg))
					? undefined
					: "Paladins",
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

const KILL_GRACE_MS = 5000;

export const killGame = async (instance: Instance) => {
	const process = processesList.value.find((p: Process) => p.instance.id === instance.id);
	if (!process) return;

	await process.child.write("kill\n").catch(() => {});

	setTimeout(async () => {
		const stillThere = processesList.value.some((p) => p.child === process.child);
		if (!stillThere) return;
		appendProcessLog("Game did not stop in time, killing launcher process", true, "launch");
		await process.child.kill().catch(() => {});
		processesList.value = processesList.value.filter((p) => p.child !== process.child);
	}, KILL_GRACE_MS);
};
