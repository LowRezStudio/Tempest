import { installMod } from "$lib/core/mods";
import { installDependenciesForPackage, offerMissingDependencies } from "$lib/mods/dependencies";
import { enqueueInstall, isGameRunning } from "$lib/mods/installQueue.svelte";
import { trackInstall } from "$lib/mods/pendingInstalls.svelte";
import { confirmReplaceMod, confirmUnverifiedMod } from "$lib/mods/ui.svelte";
import { m } from "$lib/paraglide/messages";
import { addToast } from "$lib/stores/ui.svelte";
import type { ModInstallResult } from "$lib/core/mods";
import type { DependencyMode } from "$lib/mods/dependencies";
import type { InstallChoices, QueuedInstall } from "$lib/mods/installQueue.svelte";
import type { InstallTracker } from "$lib/mods/pendingInstalls.svelte";

export type InstallOutcome =
	| { status: "installed"; result: ModInstallResult }
	/** Its files are in use (the game is running): it waits in the install queue instead. */
	| { status: "queued" }
	| { status: "cancelled" }
	| { status: "failed"; message: string };

// Refusals that mean "the game (or something) holds these files": the CLI says so for V2
// packages before touching anything; older installers only fail with the OS error.
const IN_USE_PATTERN = /being used by another process|sharing violation|\bEBUSY\b|busy or locked/i;
const filesInUse = (res: ModInstallResult) => !!res.FilesInUse || IN_USE_PATTERN.test(res.Message);

// Each install is a CLI process that verifies its package before taking the instance's mod lock,
// so several run at once; this caps how many.
const MAX_CLI_INSTALLS = Math.min(navigator.hardwareConcurrency || 4, 8);
let runningInstalls = 0;
const waitingInstalls: (() => void)[] = [];

async function limitedInstallMod(...args: Parameters<typeof installMod>) {
	if (runningInstalls < MAX_CLI_INSTALLS) runningInstalls++;
	else await new Promise<void>((resolve) => waitingInstalls.push(resolve));
	try {
		return await installMod(...args);
	} finally {
		const next = waitingInstalls.shift();
		if (next) next();
		else runningInstalls--;
	}
}

// Installs files side by side, each with its placeholder row from the start, and toasts one
// summary for those that installed. Resolves to how many did.
export async function installModFiles(gamePath: string, filePaths: string[]): Promise<number> {
	const names = filePaths.map((filePath) => filePath.split(/[/\\]/).pop() ?? filePath);
	const outcomes = await Promise.all(
		filePaths.map((filePath, i) => installModFile(gamePath, filePath, names[i])),
	);
	const installed = outcomes.flatMap((outcome, i) =>
		outcome.status === "installed" ? [outcome.result.Mod?.Name ?? names[i]] : [],
	);
	if (installed.length === 1) {
		addToast({
			title: m.toast_mod_installed_title(),
			message: m.toast_mod_installed_message({ name: installed[0] }),
			tone: "success",
		});
	} else if (installed.length > 1) {
		addToast({
			title: m.toast_mod_installed_title(),
			message: `Successfully installed ${installed.length} mods`,
			tone: "success",
		});
	}
	return installed.length;
}

type InstallOptions = Partial<InstallChoices> & {
	tracker?: InstallTracker;
	dependencies?: DependencyMode;
	/** The install queue entry this run comes from (see installQueue.svelte.ts). */
	queued?: QueuedInstall;
	/** Install even though the game runs, instead of queueing it ("Install now"). */
	force?: boolean;
};

// The one install sequence: read the manifest and ask about dependencies the instance lacks,
// install, ask about an untrusted signer, ask about conflicts, install again accordingly. While
// it runs the mod shows as a placeholder row in the mod list (taking over `tracker` from a
// preceding download when given), filled in from the package's manifest; failures get an error
// toast, success toasts are left to the caller so batches can be summarised. Refused because
// the game holds files it would replace, it goes into the install queue with the choices made
// so far and runs again once the game has closed.
export async function installModFile(
	gamePath: string,
	filePath: string,
	displayName: string,
	options: InstallOptions = {},
): Promise<InstallOutcome> {
	const queued = options.queued;
	const tracker =
		options.tracker ??
		trackInstall(
			gamePath,
			queued
				? { name: queued.name, version: queued.version, modId: queued.modId }
				: { name: displayName.replace(/\.(tempest|zip|upk|pck)$/i, "") },
		);
	const manifest = await tracker.readPackage(filePath);
	// What the package says it needs goes in first (after a yes), so the mod works right away.
	// A repository install resolved and installed them before downloading; `skip` says so.
	const askedBefore = !!manifest && options.dependencies !== "skip";
	if (askedBefore) {
		let go: boolean;
		try {
			go = await installDependenciesForPackage(
				gamePath,
				manifest.name || displayName,
				manifest.dependencies,
				options.dependencies ?? "ask",
			);
		} catch (error: unknown) {
			await tracker.finish(false);
			const message = (error as Error).message;
			addToast({
				title: m.toast_installation_failed_title(),
				message: `${displayName}: ${message}`,
				tone: "error",
			});
			return { status: "failed", message };
		}
		if (!go) {
			await tracker.finish(false);
			return { status: "cancelled" };
		}
	}
	// While the game runs, the CLI checks the package (so every question is asked now) and stops
	// before writing anything; the install then waits in the queue for the game to close.
	const defer = !options.force && isGameRunning(gamePath);
	const result = await runInstall(
		gamePath,
		filePath,
		displayName,
		{
			replace: options.replace ?? false,
			stack: options.stack ?? false,
			allowUntrusted: options.allowUntrusted ?? false,
		},
		defer,
	);
	if (result.status === "in-use") {
		// The manifest's fields are empty strings when missing.
		const first = (...values: (string | undefined)[]) => values.find((v) => !!v);
		const name =
			first(manifest?.name, queued?.name) ??
			displayName.replace(/\.(tempest|zip|upk|pck)$/i, "");
		try {
			await enqueueInstall(
				{
					gamePath,
					file: filePath,
					name,
					version: first(manifest?.version, queued?.version),
					author: first(manifest?.authors.join(", "), queued?.author),
					modId: first(manifest?.id, queued?.modId),
					...result.choices,
					lockedFiles: result.lockedFiles,
				},
				queued?.id,
			);
		} catch (error: unknown) {
			await tracker.finish(false);
			const message = `${result.message} (${(error as Error).message})`;
			addToast({
				title: m.toast_installation_failed_title(),
				message: `${displayName}: ${message}`,
				tone: "error",
			});
			return { status: "failed", message };
		}
		await tracker.finish(false);
		if (!queued) {
			addToast({
				title: m.install_queue_added_title(),
				message: m.install_queue_added_message({ name }),
				tone: "info",
			});
		}
		return { status: "queued" };
	}
	const outcome = result;
	await tracker.finish(
		outcome.status === "installed",
		outcome.status === "installed" ? outcome.result.Mod?.Id : undefined,
	);
	// The manifest could not be read beforehand, yet the CLI found dependencies missing: offer
	// them now.
	if (
		!askedBefore &&
		outcome.status === "installed" &&
		outcome.result.MissingDependencies?.length
	) {
		await offerMissingDependencies(
			gamePath,
			outcome.result.Mod?.Name ?? displayName,
			outcome.result.MissingDependencies.map((d) => ({
				id: d.Id,
				requirement: d.Requirement,
			})),
		);
	}
	return outcome;
}

type RunResult =
	| Exclude<InstallOutcome, { status: "queued" }>
	| { status: "in-use"; message: string; lockedFiles: string[]; choices: InstallChoices };

// Install, then the questions the CLI's answer raises (untrusted signer, conflicts) and the
// install again with the answers. `choices` are answers given before (a queued install);
// `defer` stops before anything is written (the game runs), handing back "in-use" to queue it.
async function runInstall(
	gamePath: string,
	filePath: string,
	displayName: string,
	choices: InstallChoices,
	defer: boolean,
): Promise<RunResult> {
	let { replace, stack, allowUntrusted } = choices;
	try {
		let res = await limitedInstallMod(
			gamePath,
			filePath,
			replace,
			allowUntrusted,
			stack,
			defer,
		);
		if (res.Unverified) {
			const confirmed = await confirmUnverifiedMod(
				displayName,
				res.Signer ?? "",
				!!res.KeyListOffline,
			);
			if (!confirmed) return { status: "cancelled" };
			allowUntrusted = true;
			res = await limitedInstallMod(gamePath, filePath, replace, true, stack, defer);
		}
		if (res.Conflict) {
			const choice = await confirmReplaceMod(
				displayName,
				res.IsModConflict,
				res.ConflictingMods ?? [],
				res.NewModName ?? displayName,
			);
			if (choice === "stack") {
				replace = false;
				stack = true;
			} else if (choice === "replace") {
				replace = true;
				stack = false;
			} else return { status: "cancelled" };
			res = await limitedInstallMod(
				gamePath,
				filePath,
				replace,
				allowUntrusted,
				stack,
				defer,
			);
		}
		if (res.Success) return { status: "installed", result: res };
		const message = res.Message || m.toast_installation_failed_unknown();
		if (res.Deferred || filesInUse(res)) {
			return {
				status: "in-use",
				message,
				lockedFiles: res.LockedFiles ?? [],
				choices: { replace, stack, allowUntrusted },
			};
		}
		addToast({
			title: m.toast_installation_failed_title(),
			message: `${displayName}: ${message}`,
			tone: "error",
		});
		return { status: "failed", message };
	} catch (error: unknown) {
		const message = (error as Error).message ?? m.toast_installation_failed_internal();
		addToast({
			title: m.toast_installation_failed_title(),
			message: `${displayName}: ${message}`,
			tone: "error",
		});
		return { status: "failed", message };
	}
}
