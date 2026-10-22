import { installMod } from "$lib/core/mods";
import { installDependenciesForPackage, offerMissingDependencies } from "$lib/mods/dependencies";
import { trackInstall } from "$lib/mods/pendingInstalls.svelte";
import { confirmReplaceMod, confirmUnverifiedMod } from "$lib/mods/ui.svelte";
import { m } from "$lib/paraglide/messages";
import { addToast } from "$lib/stores/ui.svelte";
import type { ModInstallResult } from "$lib/core/mods";
import type { DependencyMode } from "$lib/mods/dependencies";
import type { InstallTracker } from "$lib/mods/pendingInstalls.svelte";

export type InstallOutcome =
	| { status: "installed"; result: ModInstallResult }
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

// The one install sequence: read the manifest and ask about dependencies the instance lacks,
// install, ask about an untrusted signer, ask about conflicts, install again accordingly. While
// it runs the mod shows as a placeholder row in the mod list (taking over `tracker` from a
// preceding download when given), filled in from the package's manifest; failures get an error
// toast, success toasts are left to the caller so batches can be summarised.
export async function installModFile(
	gamePath: string,
	filePath: string,
	displayName: string,
	options: { replace?: boolean; tracker?: InstallTracker; dependencies?: DependencyMode } = {},
): Promise<InstallOutcome> {
	const tracker =
		options.tracker ??
		trackInstall(gamePath, { name: displayName.replace(/\.(tempest|zip|upk|pck)$/i, "") });
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
	const outcome = await runInstall(gamePath, filePath, displayName, options.replace ?? false);
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

async function runInstall(
	gamePath: string,
	filePath: string,
	displayName: string,
	replace: boolean,
): Promise<InstallOutcome> {
	try {
		let allowUntrusted = false;
		let res = await limitedInstallMod(gamePath, filePath, replace, false);
		if (res.Unverified) {
			const confirmed = await confirmUnverifiedMod(
				displayName,
				res.Signer ?? "",
				!!res.KeyListOffline,
			);
			if (!confirmed) return { status: "cancelled" };
			allowUntrusted = true;
			res = await limitedInstallMod(gamePath, filePath, replace, true);
		}
		if (res.Conflict) {
			const choice = await confirmReplaceMod(
				displayName,
				res.IsModConflict,
				res.ConflictingMods ?? [],
				res.NewModName ?? displayName,
			);
			if (choice === "stack") {
				res = await limitedInstallMod(gamePath, filePath, false, allowUntrusted, true);
			} else if (choice === "replace") {
				res = await limitedInstallMod(gamePath, filePath, true, allowUntrusted);
			} else return { status: "cancelled" };
		}
		if (res.Success) return { status: "installed", result: res };
		const message = res.Message || m.toast_installation_failed_unknown();
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
