import { downloadModPackage } from "$lib/mods/download";
import { currentPreparedInstance, handleModFileDrop } from "$lib/mods/drop.svelte";
import { trackInstall } from "$lib/mods/pendingInstalls.svelte";
import { installFromRepo } from "$lib/mods/repoInstall";
import { m } from "$lib/paraglide/messages";
import { invoke, listen } from "$lib/platform/api";
import { getRepoMod, getRepoVersions, repoDownloadUrl } from "$lib/queries/repo";
import { addToast } from "$lib/stores/ui.svelte";

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

// Installs the requested version (the newest published one when none is given). With an instance
// page open the mod goes straight into its list, placeholder row first; otherwise the download
// is handed to the drop flow, which asks for an instance.
export async function handleDeepLink(url: string): Promise<void> {
	const link = parseInstallLink(url);
	if (!link) return;
	const instance = currentPreparedInstance();
	const tracker =
		instance && trackInstall(instance.path, { name: link.modId, version: link.version }, true);
	try {
		const [mod, version] = await Promise.all([
			getRepoMod(link.modId).catch(() => null),
			link.version ?? latestApprovedVersion(link.modId),
		]);
		const name = mod?.name ?? link.modId;
		if (instance && tracker) {
			tracker.setInfo({ name, version, author: mod?.owner.displayName });
			await installFromRepo(instance.path, link.modId, version, name, { tracker });
			return;
		}
		const file = await downloadModPackage(
			repoDownloadUrl(link.modId, version),
			`${link.modId}-${version}.tempest`,
		);
		handleModFileDrop([file]);
	} catch (error: unknown) {
		void tracker?.finish(false);
		addToast({
			title: m.toast_installation_failed_title(),
			message: `${link.modId}: ${(error as Error).message}`,
			tone: "error",
		});
	}
}

async function latestApprovedVersion(modId: string) {
	const versions = (await getRepoVersions(modId)).filter((v) => v.status === "approved");
	const version = versions[0]?.version;
	if (!version) throw new Error(m.repo_no_published_version());
	return version;
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
