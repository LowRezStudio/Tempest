import { installMod, modFilesInUse } from "$lib/core/mods";
import { installDependenciesForPackage, offerMissingDependencies } from "$lib/mods/dependencies";
import { enqueueInstall, isGameRunning } from "$lib/mods/installQueue.svelte";
import { trackInstall } from "$lib/mods/pendingInstalls.svelte";
import { confirmReplaceMod, confirmUnverifiedMod } from "$lib/mods/ui.svelte";
import { m } from "$lib/paraglide/messages";
import { addToast } from "$lib/stores/ui.svelte";
import type { ModInstallResult, PatchProgress } from "$lib/core/mods";
import type { DependencyMode } from "$lib/mods/dependencies";
import type { InstallChoices, QueuedInstall } from "$lib/mods/installQueue.svelte";
import type { InstallTracker } from "$lib/mods/pendingInstalls.svelte";

export type InstallOutcome =
	| { status: "installed"; result: ModInstallResult }
	/** Its files are in use (the game is running): it waits in the install queue instead. */
	| { status: "queued" }
	| { status: "cancelled" }
	| { status: "failed"; message: string };

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
	/**
	 * Queue a mod that is only Lua scripts while the game runs too, instead of installing and
	 * hot reloading it: updates nobody asked for right now must not restart a mod mid-match.
	 */
	waitForGame?: boolean;
	/**
	 * The package was downloaded from the mod repository, whose moderators reviewed it: an
	 * unvetted signer is asked about in those terms instead of the local-file warning.
	 */
	fromRepository?: boolean;
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
	try {
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
		// before writing anything; the install then waits in the queue for the game to close, unless
		// the mod is only Lua scripts, which the game reloads.
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
			tracker.setPatching,
			options.fromRepository ?? false,
			options.waitForGame ?? false,
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
						fromRepository: options.fromRepository ?? false,
						...result.choices,
						lockedFiles: result.lockedFiles,
						hotReloadable: result.hotReloadable,
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
			await tracker.finish(false, undefined, true);
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
		// The manifest could not be read beforehand, yet the CLI found dependencies missing: offer
		// them now.
		let successful = outcome.status === "installed";
		try {
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
		} catch (error) {
			successful = false;
			throw error;
		} finally {
			await tracker.finish(
				outcome.status === "installed",
				outcome.status === "installed" ? outcome.result.Mod?.Id : undefined,
				successful,
			);
		}
		return outcome;
	} catch (error) {
		await tracker.finish(false);
		throw error;
	}
}

type RunResult =
	| Exclude<InstallOutcome, { status: "queued" }>
	| {
			status: "in-use";
			message: string;
			lockedFiles: string[];
			choices: InstallChoices;
			hotReloadable: boolean;
	  };

// Install, then the questions the CLI's answer raises (untrusted signer, conflicts) and the
// install again with the answers. `choices` are answers given before (a queued install);
// `defer` stops before anything is written (the game runs), handing back "in-use" to queue it.
async function runInstall(
	gamePath: string,
	filePath: string,
	displayName: string,
	choices: InstallChoices,
	defer: boolean,
	onProgress: PatchProgress,
	fromRepository: boolean,
	waitForGame: boolean,
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
			onProgress,
		);
		if (res.Unverified) {
			const confirmed = await confirmUnverifiedMod(
				displayName,
				res.Signer ?? "",
				!!res.KeyListOffline,
				fromRepository,
			);
			if (!confirmed) return { status: "cancelled" };
			allowUntrusted = true;
			res = await limitedInstallMod(
				gamePath,
				filePath,
				replace,
				true,
				stack,
				defer,
				onProgress,
			);
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
				onProgress,
			);
		}
		if (res.Deferred && res.HotReloadable && !waitForGame) {
			res = await limitedInstallMod(
				gamePath,
				filePath,
				replace,
				allowUntrusted,
				stack,
				false,
				onProgress,
			);
		}
		if (res.Success) return { status: "installed", result: res };
		const message = res.Message || m.toast_installation_failed_unknown();
		if (res.Deferred || modFilesInUse(res)) {
			return {
				status: "in-use",
				message,
				lockedFiles: res.LockedFiles ?? [],
				choices: { replace, stack, allowUntrusted },
				hotReloadable: !!res.HotReloadable,
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
