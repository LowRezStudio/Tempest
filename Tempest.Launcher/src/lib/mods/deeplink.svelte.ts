import { goto } from "$app/navigation";
import { currentPreparedInstance } from "$lib/mods/drop.svelte";
import { trackInstall } from "$lib/mods/pendingInstalls.svelte";
import { installFromRepo } from "$lib/mods/repoInstall";
import { pickCompatibleInstance } from "$lib/mods/ui.svelte";
import { m } from "$lib/paraglide/messages";
import { invoke, listen } from "$lib/platform/api";
import { getRepoMod, getRepoVersions } from "$lib/queries/repo";
import { addToast } from "$lib/stores/ui.svelte";
import { describeCompat, isCompatible } from "$lib/utils/version";
import type { InstallTracker } from "$lib/mods/pendingInstalls.svelte";
import type { RepoCompat } from "$lib/queries/repo";
import type { Instance } from "$lib/types/instance";

// `tempest://install?mod=<id>&version=<v>` → { modId, version } or null.
export function parseInstallLink(url: string): { modId: string; version?: string } | null {
	let parsed: URL;
	try {
		parsed = new URL(url);
	} catch {
		return null;
	}
	if (parsed.protocol !== "tempest:") return null;
	const action = parsed.hostname || parsed.pathname.replace(/^\/+/, "").split("/")[0];
	if (action !== "install") return null;
	const modId = parsed.searchParams.get("mod") ?? "";
	if (!/^[a-z0-9][a-z0-9._-]{1,63}$/.test(modId)) return null;
	const version = parsed.searchParams.get("version") ?? undefined;
	if (version !== undefined && !/^[0-9A-Za-z.+-]{1,64}$/.test(version)) return null;
	return { modId, version };
}

// Installs the requested version (without one, the newest published one for the open instance's
// patch). With an instance page open whose patch the version supports, the mod goes straight into
// its list; otherwise an instance picker asks where to put it, supported patches first.
export async function handleDeepLink(url: string): Promise<void> {
	const link = parseInstallLink(url);
	if (!link) return;
	const current = currentPreparedInstance();
	let tracker: InstallTracker | undefined;
	try {
		const [mod, versions] = await Promise.all([
			getRepoMod(link.modId).catch(() => null),
			// A requested version can still install when the list is unavailable, just unchecked.
			getRepoVersions(link.modId).catch((error: unknown) => {
				if (!link.version) throw error;
				return [];
			}),
		]);
		const name = mod?.name ?? link.modId;
		const approved = versions.filter((v) => v.status === "approved");
		const chosen = link.version
			? versions.find((v) => v.version === link.version)
			: (approved.find((v) => isCompatible(v.compat, current?.version)) ?? approved[0]);
		const version = link.version ?? chosen?.version;
		if (!version) throw new Error(m.repo_no_published_version());

		const fitsCurrent = current && (!chosen || isCompatible(chosen.compat, current.version));
		const instance = fitsCurrent
			? current
			: await pickCompatibleInstance({
					compat: chosen?.compat ?? null,
					hint: pickerHint(name, version, chosen?.compat, current),
				});
		if (!instance) return;
		if (instance.id !== current?.id) void goto(`/instance/${instance.id}`);

		tracker = trackInstall(instance.path, { name, version, modId: link.modId }, true);
		tracker.setInfo({ author: mod?.owner.displayName });
		await installFromRepo(instance.path, link.modId, version, name, {
			tracker,
			sha256: chosen?.sha256,
		});
	} catch (error: unknown) {
		void tracker?.finish(false);
		addToast({
			title: m.toast_installation_failed_title(),
			message: `${link.modId}: ${(error as Error).message}`,
			tone: "error",
		});
	}
}

function pickerHint(
	name: string,
	version: string,
	compat: RepoCompat | undefined,
	current: Instance | undefined,
): string | undefined {
	const supported = compat && describeCompat(compat);
	if (!supported || supported === "all") return undefined;
	return current
		? m.select_instance_incompatible_hint({
				name,
				version,
				compat: supported,
				patch: current.version ?? "",
			})
		: m.select_instance_compat_hint({ name, version, compat: supported });
}

const seen = new Set<string>();

function handleUrls(urls: string[]) {
	for (const url of urls) {
		// A single launch can queue the same URL through both argv and open-url.
		if (seen.has(url)) continue;
		seen.add(url);
		void handleDeepLink(url);
	}
}

async function importPendingDeepLinks() {
	try {
		const urls = await invoke("app:take-pending-deep-links");
		handleUrls(urls);
	} catch (error: unknown) {
		console.error("Failed to import pending deep links:", error);
	}
}

$effect.root(() => {
	let unlisten: (() => void) | undefined;
	void listen("deep-link", () => {
		void importPendingDeepLinks();
	}).then((fn) => {
		unlisten = fn;
		void importPendingDeepLinks();
	});
	return () => unlisten?.();
});
