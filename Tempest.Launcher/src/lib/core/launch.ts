import { invoke } from "@tauri-apps/api/core";
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

const sessionNumbers = new Map<string, number>();

const nextSessionNumber = (instanceId: string) => {
	const next = (sessionNumbers.get(instanceId) ?? 0) + 1;
	sessionNumbers.set(instanceId, next);
	return next;
};

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

	const launchState = { childPid: undefined as number | undefined, closed: false };
	command.on("close", () => {
		launchState.closed = true;
		if (launchState.childPid !== undefined) {
			processesList.value = processesList.value.filter(
				(p) => p.child.pid !== launchState.childPid,
			);
		}
	});

	const child = await command.spawn();
	launchState.childPid = child.pid;
	if (launchState.closed) return;

	const process: Process = {
		status: "on",
		sessionNumber: nextSessionNumber(instance.id),
		startedAt: Date.now(),
		child,
		command,
		instance,
	};

	processesList.value = [...processesList.value, process];
	watchForExit(process);
};

const KILL_GRACE_MS = 5000;

export const killGame = async (instance: Instance) => {
	const windows = processesList.value.filter((p: Process) => p.instance.id === instance.id);
	await Promise.all(windows.map((p) => stopGameWindow(p)));
};

export const stopGameWindow = async (process: Process) => {
	if (process.status === "stopping") return;
	process.status = "stopping";

	await process.child.write("kill\n").catch(() => {});

	setTimeout(async () => {
		const stillThere = processesList.value.some((p) => p.child === process.child);
		if (!stillThere) return;
		appendProcessLog("Game did not stop in time, killing launcher process", true, "launch");
		await process.child.kill().catch(() => {});
		processesList.value = processesList.value.filter((p) => p.child !== process.child);
	}, KILL_GRACE_MS);
};

const EXIT_POLL_MS = 2000;

/**
 * Removes a game whose launcher process has exited even if the shell never reports `close`,
 * which happens while leftover child processes still hold its output pipes.
 * @param process The launched game to watch.
 */
function watchForExit(process: Process): void {
	const isTracked = () => processesList.value.some((p) => p.child === process.child);
	const timer = setInterval(async () => {
		const running =
			isTracked() &&
			(await invoke<boolean>("is_process_running", { pid: process.child.pid }).catch(
				() => true,
			));
		if (running) return;
		clearInterval(timer);
		processesList.value = processesList.value.filter((p) => p.child !== process.child);
	}, EXIT_POLL_MS);
}
