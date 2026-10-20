import { downloadRemoteMod } from "$lib/core/mods";
import { handleModFileDrop } from "$lib/mods/drop.svelte";
import { m } from "$lib/paraglide/messages";
import { invoke, listen } from "$lib/platform/api";
import { getRepoVersions, repoDownloadUrl } from "$lib/queries/repo";
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

// Downloads the requested version (the newest published one when none is given) and hands the
// file to the same flow a dropped `.tempest` file takes: install into the open instance, or ask
// which instance.
export async function handleDeepLink(url: string): Promise<void> {
	const link = parseInstallLink(url);
	if (!link) return;
	try {
		let version = link.version;
		if (!version) {
			const versions = (await getRepoVersions(link.modId)).filter(
				(v) => v.status === "approved",
			);
			version = versions[0]?.version;
			if (!version) throw new Error(m.repo_no_published_version());
		}
		const file = await downloadRemoteMod(
			repoDownloadUrl(link.modId, version),
			`${link.modId}-${version}.tempest`,
		);
		handleModFileDrop([file]);
	} catch (error: unknown) {
		addToast({
			title: m.toast_installation_failed_title(),
			message: `${link.modId}: ${(error as Error).message}`,
			tone: "error",
		});
	}
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
