import { fetch as httpFetch, invoke, listen, resolveResource } from "$lib/platform/api";
import { getQueryClient } from "$lib/queries/client";
import { appendProcessLogs } from "$lib/stores/processes.svelte";
import { modRepositoryURL } from "$lib/stores/settings.svelte";
import { reportAntivirusBlocks } from "./antivirus.svelte";
import { createCommand } from "./command";
import type { Instance } from "$lib/types/instance";

const REMOTE_CORE_URL =
	"https://github.com/LowRezStudio/TgMod/releases/download/1.0.0/Tempest.Core.Tempest";
const LATEST_CORE_API = "https://api.github.com/repos/LowRezStudio/TgMod/releases/latest";
const CORE_VERSION_KEY = "tempest_core_remote_version";

export async function downloadRemoteMod(
	url: string,
	filename: string,
	onProgress?: (received: number, total: number | null) => void,
	cache?: { modId: string; version: string; sha256?: string },
): Promise<string> {
	// Host-side download: the bytes stream straight to a temp file in the main
	// process (no renderer memory, no CORS) and we only pass the path around.
	if (!onProgress) return invoke("mods:download-to-temp", { url, filename, cache });
	const progressId = crypto.randomUUID();
	const unlisten = await listen("mods:download-progress", ({ id, received, total }) => {
		if (id === progressId) onProgress(received, total);
	});
	try {
		return await invoke("mods:download-to-temp", { url, filename, progressId, cache });
	} finally {
		unlisten();
	}
}
export type ModAuthor = {
	Name: string;
	Link: string;
	Avatar?: string;
};

export type ModRecord = {
	Id: string;
	Name: string;
	Author: string;
	Authors?: ModAuthor[];
	Version: string;
	Enabled: boolean;
	Kind: string;
	OriginalPath: string;
	InstalledFiles: string[];
	OwnedFiles: string[];
	Readme?: string;
	ReadmeContent?: string;
};

export type ModConflictInfo = {
	ModId: string;
	ModName: string;
	ModVersion?: string;
	ConflictingFiles: string[];
};

export type ModInstallResult = {
	Success: boolean;
	Message: string;
	Mod?: ModRecord;
	Conflict?: boolean;
	IsModConflict?: boolean;
	/** Code signed by a key that is not trusted; install again with allowUntrusted once the user agrees. */
	Unverified?: boolean;
	/** Who signed an Unverified mod, "user id [fingerprint]". */
	Signer?: string;
	/** The published key lists could not be reached, so only the built-in keys were trusted. */
	KeyListOffline?: boolean;
	ConflictingMods?: ModConflictInfo[];
	NewModName?: string;
};

export type ModListResult = {
	Mods: ModRecord[];
};

/** Versions that get Tempest Core instead of Tempest Console. */
const CORE_MOD_VERSIONS = new Set(["0.56", "0.57"]);

/** Versions that should skip auto-mod installation entirely. */
const SKIP_AUTO_MOD_VERSIONS = new Set(["8.1"]);

export const installAutoMods = async (instance: Instance): Promise<void> => {
	const gamePath = instance?.path;
	if (!gamePath) return;

	if (instance.version && SKIP_AUTO_MOD_VERSIONS.has(instance.version)) return;

	const isCoreVersion = !!(instance.version && CORE_MOD_VERSIONS.has(instance.version));

	if (isCoreVersion) {
		// Fetch latest Tempest Core from GitHub so you can update it without rebuilding the launcher
		try {
			appendProcessLogs([`Fetching Tempest Core from ${REMOTE_CORE_URL}`], false, "mods");
			const modFile = await downloadRemoteMod(REMOTE_CORE_URL, "Tempest.Core.Tempest");
			await installMod(gamePath, modFile, true);
		} catch (error) {
			console.error("Failed to fetch remote Tempest Core, falling back to bundled:", error);
			try {
				const fallback = await resolveResource("mods/Tempest Core.tempest");
				await installMod(gamePath, fallback, true);
			} catch (error) {
				console.error("Failed to install fallback Tempest Core:", error);
			}
		}
	} else {
		try {
			const modFile = await resolveResource("mods/Tempest Console.tempest");
			await installMod(gamePath, modFile, true);
		} catch (error) {
			console.error("Failed to install Tempest Console:", error);
		}
	}

	void getQueryClient()?.invalidateQueries({ queryKey: ["mods", gamePath] });
};

export const checkForCoreUpdatesAndInstall = async (instances: Instance[]): Promise<void> => {
	try {
		const res = await httpFetch(LATEST_CORE_API, {
			method: "GET",
			headers: { Accept: "application/vnd.github+json" },
		});
		if (!res.ok) return;
		const data = await res.json();
		const tag: string = data.tag_name;
		if (!tag) return;
		const stored = localStorage.getItem(CORE_VERSION_KEY);
		// If we've already applied this tag, skip
		if (stored === tag) return;
		const asset =
			(data.assets as Array<{ name: string; browser_download_url: string }>)?.find(
				(a) => a.name === "Tempest.Core.Tempest",
			) ??
			(data.assets as Array<{ name: string; browser_download_url: string }>)?.find(
				(a) => a.name === "Tempest Core.tempest",
			) ??
			(data.assets as Array<{ name: string; browser_download_url: string }>)?.[0];
		if (!asset?.browser_download_url) return;
		const url = asset.browser_download_url;
		appendProcessLogs([`New Tempest Core ${tag} found, downloading...`], false, "mods");
		const modFile = await downloadRemoteMod(url, "Tempest.Core.Tempest");
		// Verify corresponding builds: only Core versions (0.56/0.57) get the update
		const coreInstances = instances.filter(
			(i) => i.version && CORE_MOD_VERSIONS.has(i.version),
		);
		if (coreInstances.length === 0) {
			localStorage.setItem(CORE_VERSION_KEY, tag);
			return;
		}
		for (const inst of coreInstances) {
			try {
				appendProcessLogs(
					[`Updating ${inst.label} (${inst.version}) to Core ${tag}`],
					false,
					"mods",
				);
				await installMod(inst.path, modFile, true);
			} catch (error) {
				console.error(`Failed to update ${inst.label}:`, error);
			}
		}
		localStorage.setItem(CORE_VERSION_KEY, tag);
		appendProcessLogs(
			[`Tempest Core updated to ${tag} for ${coreInstances.length} instance(s)`],
			false,
			"mods",
		);
		// Also refresh mod caches
		for (const inst of coreInstances) {
			void getQueryClient()?.invalidateQueries({ queryKey: ["mods", inst.path] });
		}
	} catch (error) {
		console.error("Core update check failed:", error);
	}
};

export const listMods = async (gamePath: string): Promise<ModRecord[]> => {
	const args = ["mod", "list", gamePath, "--json"];
	appendProcessLogs([`Running command: tempest-cli ${args.join(" ")}`], false, "mods");
	const res = await createCommand(args).execute();

	if (res.stdout) {
		appendProcessLogs(res.stdout.split("\n").filter(Boolean), false, "mods");
	}
	if (res.stderr) {
		appendProcessLogs(res.stderr.split("\n").filter(Boolean), true, "mods");
	}

	if (res.code !== 0) return [];
	try {
		const parsed = JSON.parse(res.stdout) as ModListResult;
		return parsed.Mods || [];
	} catch (error) {
		console.error("Failed to parse mods list:", error);
		return [];
	}
};

export const installMod = async (
	gamePath: string,
	modFile: string,
	replace = false,
	allowUntrusted = false,
	stack = false,
): Promise<ModInstallResult> => {
	const args = ["mod", "install", gamePath, modFile];
	if (replace) args.push("--replace");
	if (stack) args.push("--stack");
	if (allowUntrusted) args.push("--allow-untrusted");
	args.push("--json");

	appendProcessLogs([`Running command: tempest-cli ${args.join(" ")}`], false, "mods");
	const res = await createCommand(args).execute();

	if (res.stdout) {
		appendProcessLogs(res.stdout.split("\n").filter(Boolean), false, "mods");
	}
	if (res.stderr) {
		appendProcessLogs(res.stderr.split("\n").filter(Boolean), true, "mods");
	}

	try {
		return JSON.parse(res.stdout) as ModInstallResult;
	} catch (error) {
		console.error("Failed to parse install mod result:", error, res.stdout, res.stderr);
		return { Success: false, Message: "Internal error parsing CLI output" };
	}
};

export const removeMod = async (gamePath: string, modName: string): Promise<ModInstallResult> => {
	const args = ["mod", "remove", gamePath, modName, "--json"];
	appendProcessLogs([`Running command: tempest-cli ${args.join(" ")}`], false, "mods");
	const res = await createCommand(args).execute();

	if (res.stdout) {
		appendProcessLogs(res.stdout.split("\n").filter(Boolean), false, "mods");
	}
	if (res.stderr) {
		appendProcessLogs(res.stderr.split("\n").filter(Boolean), true, "mods");
	}

	try {
		return JSON.parse(res.stdout) as ModInstallResult;
	} catch (error) {
		console.error("Failed to parse remove mod result:", error, res.stdout, res.stderr);
		return { Success: false, Message: "Internal error parsing CLI output" };
	}
};

export const renameMod = async (
	gamePath: string,
	oldName: string,
	newName: string,
): Promise<ModInstallResult> => {
	const args = ["mod", "rename", gamePath, oldName, newName, "--json"];
	appendProcessLogs([`Running command: tempest-cli ${args.join(" ")}`], false, "mods");
	const res = await createCommand(args).execute();

	if (res.stdout) {
		appendProcessLogs(res.stdout.split("\n").filter(Boolean), false, "mods");
	}
	if (res.stderr) {
		appendProcessLogs(res.stderr.split("\n").filter(Boolean), true, "mods");
	}

	try {
		return JSON.parse(res.stdout) as ModInstallResult;
	} catch (error) {
		console.error("Failed to parse rename mod result:", error, res.stdout, res.stderr);
		return { Success: false, Message: "Internal error parsing CLI output" };
	}
};

export const enableMod = async (gamePath: string, modName: string): Promise<ModInstallResult> => {
	const args = ["mod", "enable", gamePath, modName, "--json"];
	appendProcessLogs([`Running command: tempest-cli ${args.join(" ")}`], false, "mods");
	const res = await createCommand(args).execute();

	if (res.stdout) {
		appendProcessLogs(res.stdout.split("\n").filter(Boolean), false, "mods");
	}
	if (res.stderr) {
		appendProcessLogs(res.stderr.split("\n").filter(Boolean), true, "mods");
	}

	try {
		return JSON.parse(res.stdout) as ModInstallResult;
	} catch (error) {
		console.error("Failed to parse enable mod result:", error, res.stdout, res.stderr);
		return { Success: false, Message: "Internal error parsing CLI output" };
	}
};

export const disableMod = async (gamePath: string, modName: string): Promise<ModInstallResult> => {
	const args = ["mod", "disable", gamePath, modName, "--json"];
	appendProcessLogs([`Running command: tempest-cli ${args.join(" ")}`], false, "mods");
	const res = await createCommand(args).execute();

	if (res.stdout) {
		appendProcessLogs(res.stdout.split("\n").filter(Boolean), false, "mods");
	}
	if (res.stderr) {
		appendProcessLogs(res.stderr.split("\n").filter(Boolean), true, "mods");
	}

	try {
		return JSON.parse(res.stdout) as ModInstallResult;
	} catch (error) {
		console.error("Failed to parse disable mod result:", error, res.stdout, res.stderr);
		return { Success: false, Message: "Internal error parsing CLI output" };
	}
};

export const reloadModDll = async (gamePath: string, modId: string): Promise<ModInstallResult> => {
	const args = ["mod", "reload-dll", gamePath, modId, "--json"];
	appendProcessLogs([`Running command: tempest-cli ${args.join(" ")}`], false, "mods");
	const res = await createCommand(args).execute();
	reportAntivirusBlocks(res.stderr);

	if (res.stdout) {
		appendProcessLogs(res.stdout.split("\n").filter(Boolean), false, "mods");
	}
	if (res.stderr) {
		appendProcessLogs(res.stderr.split("\n").filter(Boolean), true, "mods");
	}

	try {
		return JSON.parse(res.stdout) as ModInstallResult;
	} catch (error) {
		console.error("Failed to parse reload-dll mod result:", error, res.stdout, res.stderr);
		return { Success: false, Message: "Internal error parsing CLI output" };
	}
};

// ---- Mod cooker: signing and repository publishing (via the CLI) ----

type CliResult = { code: number | null; signal: string | null; stdout: string; stderr: string };

export const describeError = (error: unknown): string =>
	error instanceof Error ? error.message : String(error);

const runCli = async (args: string[], label: string): Promise<CliResult> => {
	appendProcessLogs([`Running command: tempest-cli ${args.join(" ")}`], false, "mods");
	const res = await createCommand(args).execute();
	if (res.stdout) appendProcessLogs(res.stdout.split("\n").filter(Boolean), false, "mods");
	if (res.stderr) appendProcessLogs(res.stderr.split("\n").filter(Boolean), true, "mods");
	if (res.code !== 0) {
		const detail = res.stderr.trim() || res.stdout.trim() || `exit ${res.code}`;
		throw new Error(`${label}: ${detail}`);
	}
	return res;
};

// Signs a .tempest package in place with the default signing key; returns the
// signer description ("user id [fingerprint]"). Throws SigningKeyMissingError
// when this machine has no signing key yet — the UI asks for a key name, calls
// generateSigningKey, then signs again.
export class SigningKeyMissingError extends Error {
	constructor(message = "no signing key on this machine") {
		super(message);
		this.name = "SigningKeyMissing";
	}
}

const KEY_MISSING_PATTERN =
	/no signing key|could not find file|no such file|does not exist|not found/i;

// True when generation failed because a signing key is already on this machine.
export const isKeyExistsError = (message: string): boolean => /already exists/i.test(message);

export const signModPackage = async (packagePath: string): Promise<string> => {
	appendProcessLogs([`Running command: tempest-cli mod sign ${packagePath}`], false, "mods");
	const res = await createCommand(["mod", "sign", packagePath]).execute();
	if (res.stdout) appendProcessLogs(res.stdout.split("\n").filter(Boolean), false, "mods");
	if (res.stderr) appendProcessLogs(res.stderr.split("\n").filter(Boolean), true, "mods");
	if (res.code !== 0) {
		const detail = res.stderr.trim() || res.stdout.trim() || `exit ${res.code}`;
		if (KEY_MISSING_PATTERN.test(detail)) throw new SigningKeyMissingError(detail);
		throw new Error(`signing failed: ${detail}`);
	}
	// "Signed '<path>' with <user id> [<fingerprint>]"
	const signer = /^Signed '.*' with (.+)$/m.exec(res.stdout)?.[1]?.trim();
	return signer ?? "";
};

// Generates the default signing key with the user-chosen identity, trusted on
// this machine; `mod sign` picks it up afterwards.
export const generateSigningKey = async (id: string): Promise<void> => {
	await runCli(["mod", "gen-key", "--id", id], "signing key generation failed");
};

// Signs arbitrary text with the default signing key (the CLI's `mod sign-text`)
// and returns the armored PGP signature. Used to prove key ownership: paste the
// signature on your account page of the mod repository.
export const signText = async (text: string): Promise<string> => {
	const res = await runCli(["mod", "sign-text", text], "signing failed");
	return res.stdout.trim();
};

export type RepoPublishListing = {
	patches: string[];
	rangeFrom?: string;
	rangeTo?: string;
	summary: string;
	tags: string;
	changelog?: string;
	repository?: string;
};

export type RepoPublishResult = {
	state: string;
	modId?: string;
	version?: string;
	errors: string[];
	warnings: string[];
};

// Publishes a signed .tempest package to the configured mod repository.
export const repoPublish = async (
	packagePath: string,
	listing: RepoPublishListing,
): Promise<RepoPublishResult> => {
	const args = ["mod", "publish", packagePath];
	for (const patch of listing.patches) args.push("--patch", patch);
	if (listing.rangeFrom) args.push("--from", listing.rangeFrom);
	if (listing.rangeTo) args.push("--to", listing.rangeTo);
	if (listing.summary) args.push("--summary", listing.summary);
	if (listing.tags) args.push("--tags", listing.tags);
	if (listing.changelog) args.push("--changelog", listing.changelog);
	if (listing.repository) args.push("--repository", listing.repository);
	args.push("--json");

	appendProcessLogs([`Running command: tempest-cli mod publish ${packagePath}`], false, "mods");
	// Publish against the repository the launcher browses, not just the CLI's own config.
	const res = await createCommand(args, { TEMPEST_REPO_URL: modRepositoryURL.value }).execute();
	if (res.stdout) appendProcessLogs(res.stdout.split("\n").filter(Boolean), false, "mods");
	if (res.stderr) appendProcessLogs(res.stderr.split("\n").filter(Boolean), true, "mods");

	const parse = (): RepoPublishResult | null => {
		try {
			const intent = JSON.parse(res.stdout.trim()) as {
				state?: string;
				result?: {
					modId?: string;
					version?: string;
					errors?: string[];
					warnings?: string[];
				};
			};
			if (!intent.state) return null;
			return {
				state: intent.state,
				modId: intent.result?.modId,
				version: intent.result?.version,
				errors: intent.result?.errors ?? [],
				warnings: intent.result?.warnings ?? [],
			};
		} catch {
			return null;
		}
	};

	if (res.code === 0) return parse() ?? { state: "done", errors: [], warnings: [] };
	// A "failed" inspection still prints its intent JSON; other failures only stderr.
	const failed = parse();
	if (failed) return failed;
	throw new Error(res.stderr.trim() || res.stdout.trim() || `exit ${res.code}`);
};

// Stores an API key for the mod repository (same repo.json the CLI uses).
// Throws when the repository rejects the key.
export const repoLogin = async (apiKey: string): Promise<string> => {
	const res = await createCommand(["mod", "login", apiKey], {
		TEMPEST_REPO_URL: modRepositoryURL.value,
	}).execute();
	if (res.stdout) appendProcessLogs(res.stdout.split("\n").filter(Boolean), false, "mods");
	if (res.code !== 0) throw new Error(res.stderr.trim() || `exit ${res.code}`);
	return res.stdout.trim();
};

// True when the repository rejected the credentials (needs the sign-in form,
// not a generic error).
export const isRepoAuthError = (message: string): boolean =>
	/no api key|unauthorized|401|403/i.test(message);
