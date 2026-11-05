import { launchAfterModOperations } from "$lib/mods/launch.svelte";
import { m } from "$lib/paraglide/messages";
import { Command, invoke, type Child } from "$lib/platform/api";
import { openEpicLogin } from "$lib/stores/epic.svelte";
import { hostState } from "$lib/stores/host-state.svelte";
import { instanceMap, lastLaunchedInstanceId, updateInstance } from "$lib/stores/instance.svelte";
import { loginServers } from "$lib/stores/login-servers.svelte";
import { appendProcessLog, logCommandOutput, processesList } from "$lib/stores/processes.svelte";
import {
	gamescopeArgs,
	protonPath,
	useGamescope,
	useSteamRuntime,
	winePath,
	wineRuntime,
} from "$lib/stores/settings.svelte";
import { watchAntivirusBlocks } from "./antivirus.svelte";
import { createCommand, processArgs } from "./command";
import { checkForCrash, watchGameLogFolders } from "./crash.svelte";
import { launchArgs } from "./presets";
import type { GameLogFolders } from "$ipc/contract";
import type { Instance } from "$lib/types/instance";
import type { Process } from "$lib/types/process";

const sessionNumbers = new Map<string, number>();

// Keyed by the class instances: entries of `processesList` are $state proxies, so a field set on
// a process would not be seen through the object the launch holds.
const logFolders = new WeakMap<Command<string>, GameLogFolders>();
const stopRequested = new WeakSet<Child>();

/**
 * The Epic launcher's arguments for an instance set to Epic Games.
 * @param accountId - The instance's account; the default one in Settings when absent.
 * @returns The game's arguments for the sign-in, or null when one is needed first.
 */
const epicAuthArgs = (accountId?: string): Promise<string[] | null> =>
	invoke("epic:launch-args", { accountId });

/** Arguments for the selected login server; no match keeps the official service defaults.
 * @param instance - Instance being launched.
 * @returns The login-server arguments, or an empty list for the official service.
 */
const loginServerArgs = (instance: Instance): string[] => {
	const id = instance.launchOptions?.loginServerId;
	const server = id ? loginServers.value.find((item) => item.id === id) : undefined;
	const address = server?.addresses[0];
	if (!server || !address) return [];
	const host = address.includes(":") ? `[${address}]` : address;
	return [
		"-nosplash",
		"-windowed",
		`-hostx=${host}:9000`,
		`-msa=${host}:9001`,
		`-pubkey=${server.publicKey}`,
		`-servicediscoveryurl=${server.discoveryUrl}`,
	];
};

/**
 * The exchange code is single-use and short-lived, but it has no place in a log or a report.
 * @param args - The game's arguments, as they are handed to the CLI.
 * @returns The same arguments with the exchange code masked.
 */
const redactArgs = (args: string[]): string[] =>
	args.map((arg) => (/^-auth_password=/i.test(arg) ? "-auth_password=***" : arg));

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
	logFolders?: GameLogFolders;
};

export const runningGames = hostState<RunningSnapshot[]>(RUNNING_KEY, []);

const saveRunning = () => {
	runningGames.value = processesList.value.map(
		({ child, command, instance, sessionNumber, startedAt }) => ({
			pid: child.pid,
			instanceId: instance.id,
			sessionNumber,
			startedAt,
			logFolders: logFolders.get(command),
		}),
	);
};

// A launch that never printed its log folders failed before the game started: not a crash.
const watchForCrash = (
	command: Command<string>,
	instance: Instance,
	startedAt: number,
	wasStopped: () => boolean,
) => {
	command.on("close", ({ code }) => {
		const folders = logFolders.get(command);
		if (folders && !wasStopped()) void checkForCrash(instance, folders, startedAt, code);
	});
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
		if (item.logFolders) logFolders.set(command, item.logFolders);
		watchForCrash(command, instance, item.startedAt, () => stopRequested.has(child));
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
};

export const launchGame = (instance: Instance, { noDlls = false } = {}) =>
	launchAfterModOperations(instance, (wait) => startGame(instance, noDlls, wait));

const startGame = async (instance: Instance, noDlls: boolean, wait: () => Promise<boolean>) => {
	const { path, launchOptions: options } = instance;
	const platform = options.platform ?? "Win64";
	const loginServerArgsList = loginServerArgs(instance);
	const loginServerSelected = loginServerArgsList.length > 0;
	// A Steam launch just tells the client to use its Steam session; Epic gets a
	// one-time exchange code, made fresh for this launch and never stored.
	let authArgs: string[] = [];
	do {
		await wait();
		if (!loginServerSelected && options.auth?.provider === "steam") {
			authArgs = ["-steam"];
		} else if (!loginServerSelected && options.auth?.provider === "epic") {
			const epicArgs = await epicAuthArgs(options.auth.accountId);
			if (!epicArgs) {
				// The launch cannot sign in: ask for the account rather than failing it.
				openEpicLogin(m.epic_login_required());
				return;
			}
			authArgs = epicArgs;
		}
		// Mod work may start while Epic issues its short-lived exchange code.
	} while (await wait());
	const args = [...launchArgs(instance), ...loginServerArgsList, ...authArgs];

	lastLaunchedInstanceId.value = instance.id;
	updateInstance(instance.id, { lastPlayedAt: Date.now() });

	console.log("Launching instance", instance.id, instance.version);
	console.log(instance);

	appendProcessLog(
		`Launching game client with args: ${redactArgs(processArgs(args)).join(" ")}`,
		false,
		"launch",
	);

	const wine = winePath.get();
	const runtime = wineRuntime.get();
	const proton =
		runtime === "proton" ? protonPath.get() : runtime === "wine" ? "wine" : undefined;
	const env: Record<string, string> = {
		TEMPEST_CRASH_DIAGNOSTICS: "1",
		...(wine ? { WINE: wine } : {}),
		...(proton ? { PROTON: proton } : {}),
	};

	const command = createCommand(
		[
			"launch",
			path,
			{ "--platform": platform },
			{ "--no-default-args": options.noDefaultArgs },
			{ "--gamescope": useGamescope.get() },
			{ "--gamescope-args": gamescopeArgs.get() || undefined },
			{ "--steam-runtime": useSteamRuntime.get() || undefined },
			{ "--no-dlls": noDlls },
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

	const launchState = {
		childPid: undefined as number | undefined,
		closed: false,
		blocked: false,
		process: undefined as Process | undefined,
	};
	// Without its DLLs the game would run with mods silently missing, so the launch is stopped and
	// the antivirus dialog offers to relaunch.
	watchAntivirusBlocks(command, {
		instance,
		onBlocked: () => {
			launchState.blocked = true;
			if (launchState.process) void stopGameWindow(launchState.process);
		},
	});
	command.on("close", () => {
		launchState.closed = true;
		if (launchState.childPid !== undefined) forgetProcess(launchState.childPid);
	});
	watchGameLogFolders(command, (folders) => {
		logFolders.set(command, folders);
		saveRunning();
	});
	watchForCrash(
		command,
		instance,
		Date.now(),
		() =>
			launchState.blocked ||
			(!!launchState.process && stopRequested.has(launchState.process.child)),
	);

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
	launchState.process = process;
	if (launchState.blocked) void stopGameWindow(process);
};

const KILL_GRACE_MS = 5000;

export const killGame = async (instance: Instance) => {
	const windows = processesList.value.filter((p: Process) => p.instance.id === instance.id);
	await Promise.all(windows.map((p) => stopGameWindow(p)));
};

export const stopGameWindow = async (process: Process) => {
	if (process.status === "stopping") return;
	process.status = "stopping";
	stopRequested.add(process.child);

	await process.child.sendLine("kill").catch(() => {});

	setTimeout(async () => {
		const stillThere = processesList.value.some((p) => p.child === process.child);
		if (!stillThere) return;
		appendProcessLog("Game did not stop in time, killing launcher process", true, "launch");
		await process.child.kill().catch(() => {});
		forgetProcess(process.child.pid);
	}, KILL_GRACE_MS);
};
