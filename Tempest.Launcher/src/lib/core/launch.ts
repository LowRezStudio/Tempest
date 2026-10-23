import { currentInstance, setCurrentInstance } from "../lobby/stores.svelte";
import { Command, invoke } from "../platform/api";
import { hostState } from "../stores/host-state.svelte";
import { instanceMap, lastLaunchedInstanceId } from "../stores/instance.svelte";
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
import { launchArgs } from "./presets";
import type { Instance } from "../types/instance";
import type { Process } from "../types/process";

const sessionNumbers = new Map<string, number>();

const nextSessionNumber = (instanceId: string) => {
	const next = (sessionNumbers.get(instanceId) ?? 0) + 1;
	sessionNumbers.set(instanceId, next);
	return next;
};

// The host keeps games running when the renderer reloads (Vite full reload,
// renderer crash). This registry lives in the host too, so the new page knows
// which children to reattach to; `shell:list` filters out the ones that ended.
const RUNNING_KEY = "launch:running";

type RunningSnapshot = Pick<Process, "sessionNumber" | "startedAt"> & {
	pid: number;
	instanceId: string;
};

export const runningGames = hostState<RunningSnapshot[]>(RUNNING_KEY, []);

const saveRunning = () => {
	runningGames.value = processesList.value.map(
		({ child, instance, sessionNumber, startedAt }) => ({
			pid: child.pid,
			instanceId: instance.id,
			sessionNumber,
			startedAt,
		}),
	);
};

/**
 * Children the host still runs; falls back to the registry if the host answers
 * nothing (older host build without `shell:list`).
 * @returns Live host child ids.
 */
const liveGames = async (): Promise<Set<number>> => {
	try {
		return new Set(await invoke("shell:list"));
	} catch (error) {
		console.warn("[launch] could not list host games:", error);
		return new Set(runningGames.value.map((game) => game.pid));
	}
};

/**
 * Remove a tracked game and update the reload snapshot.
 * @param pid - Host child id of the game.
 */
const forgetProcess = (pid: number) => {
	processesList.value = processesList.value.filter((p) => p.child.pid !== pid);
	saveRunning();
};

/**
 * Rebuild `processesList` from the previous renderer's snapshot, keeping only
 * children the host still has. Called once at startup (see `+layout.svelte`).
 * shortcut: a launch still in flight at reload time is not adopted (the pid
 * arrives after the snapshot), upgrade by having the host own the list.
 */
export const restoreRunningGames = async () => {
	const saved = runningGames.value.filter((item) => !!instanceMap.get()[item.instanceId]);
	const live = await liveGames();
	const restored: Process[] = [];

	for (const item of saved) {
		if (!live.has(item.pid)) continue;
		const instance = instanceMap.get()[item.instanceId];
		const { command, child } = Command.attach(item.pid);
		command.on("close", () => forgetProcess(item.pid));
		logCommandOutput(command, "launch");
		restored.push({
			status: "on",
			sessionNumber: item.sessionNumber,
			startedAt: item.startedAt,
			child,
			command,
			instance,
		});
		sessionNumbers.set(
			item.instanceId,
			Math.max(sessionNumbers.get(item.instanceId) ?? 0, item.sessionNumber),
		);
	}

	// A game that exits between `shell:list` and the attach above would stay
	// listed until the next reload: acceptable, it is a few milliseconds.
	processesList.value = [
		...processesList.value,
		...restored.filter((p) => !processesList.value.some((q) => q.child.pid === p.child.pid)),
	];
	saveRunning();

	// The lobby pointer is only good while the game it launched still runs.
	if (
		currentInstance.value &&
		!restored.some((p) => p.instance.id === currentInstance.value?.id)
	) {
		setCurrentInstance(null);
	}
};

export const launchGame = async (instance: Instance) => {
	const { path, launchOptions: options } = instance;
	const platform = options.platform ?? "Win64";
	const args = launchArgs(instance);

	lastLaunchedInstanceId.value = instance.id;

	console.log("Launching instance", instance.id, instance.version);
	console.log(instance);

	appendProcessLog(
		`Launching game client with args: ${processArgs(args).join(" ")}`,
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
			...(args.length ? ["--", ...processArgs(args)] : []),
		],
		env,
	);

	logCommandOutput(command, "launch");

	const launchState = { childPid: undefined as number | undefined, closed: false };
	command.on("close", () => {
		launchState.closed = true;
		if (launchState.childPid !== undefined) forgetProcess(launchState.childPid);
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
	saveRunning();
};

const KILL_GRACE_MS = 5000;

export const killGame = async (instance: Instance) => {
	const windows = processesList.value.filter((p: Process) => p.instance.id === instance.id);
	await Promise.all(windows.map((p) => stopGameWindow(p)));
};

export const stopGameWindow = async (process: Process) => {
	if (process.status === "stopping") return;
	process.status = "stopping";

	await process.child.sendLine("kill").catch(() => {});

	setTimeout(async () => {
		const stillThere = processesList.value.some((p) => p.child === process.child);
		if (!stillThere) return;
		appendProcessLog("Game did not stop in time, killing launcher process", true, "launch");
		await process.child.kill().catch(() => {});
		forgetProcess(process.child.pid);
	}, KILL_GRACE_MS);
};
