import { getAppVersion } from "$lib/core/appVersion";
import { listMods } from "$lib/core/mods";
import { installFromRepo } from "$lib/mods/repoInstall";
import { confirmDependencies } from "$lib/mods/ui.svelte";
import { m } from "$lib/paraglide/messages";
import { getQueryClient } from "$lib/queries/client";
import { getRepoDependencies, getRepoVersions } from "$lib/queries/repo";
import { instanceMap } from "$lib/stores/instance.svelte";
import { addToast } from "$lib/stores/ui.svelte";
import {
	compareSemver,
	describeRequirement,
	modDependencies,
	parseDependency,
	satisfies,
	tempestRequirement,
	unmetTempest,
} from "$lib/utils/deps";
import { isCompatible } from "$lib/utils/version";
import type { ModRecord } from "$lib/core/mods";
import type { RepoResolvedDependency } from "$lib/queries/repo";
import type { Dependency } from "$lib/utils/deps";

/** How installs treat what a mod needs: ask first, install without asking, or leave it alone. */
export type DependencyMode = "ask" | "auto" | "skip";

/** A dependency about to be installed, as the dialog lists it. */
export type DependencyToInstall = {
	id: string;
	name: string;
	version: string;
	sha256?: string;
	/** Who asked for it, for the dialog ("needed by Tempest Console") */
	requiredBy: string[];
};

// The patch of the instance at `gamePath`, for the repository's compatibility picks.
export function patchOf(gamePath: string): string | undefined {
	return Object.values(instanceMap.value).find((i) => i.path === gamePath)?.version;
}

async function installedMods(gamePath: string): Promise<ModRecord[]> {
	const cached = getQueryClient()?.getQueryData<ModRecord[]>(["mods", gamePath]);
	return cached ?? (await listMods(gamePath));
}

// The dependencies of `record` that no enabled installed mod satisfies (`tempest` aside).
export function missingDependencies(record: ModRecord, installed: ModRecord[]): Dependency[] {
	const deps = (record.Dependencies ?? []).map((d) => ({ id: d.Id, requirement: d.Requirement }));
	return modDependencies(deps).filter((dep) => {
		const have = installed.find((mod) => mod.Id.toLowerCase() === dep.id && mod.Enabled);
		return !have || !satisfies(have.Version, dep.requirement);
	});
}

// Enabled installed mods that depend on `modId`.
export function dependantsOf(modId: string, installed: ModRecord[]): ModRecord[] {
	const id = modId.toLowerCase();
	return installed.filter(
		(mod) =>
			mod.Enabled &&
			mod.Id.toLowerCase() !== id &&
			(mod.Dependencies ?? []).some((d) => d.Id.toLowerCase() === id),
	);
}

// Whether an item of the repository's resolution is already covered by what is installed.
function covered(item: RepoResolvedDependency, installed: ModRecord[]): boolean {
	const have = installed.find((mod) => mod.Id.toLowerCase() === item.id && mod.Enabled);
	return !!have && item.requirements.every((r) => satisfies(have.Version, r));
}

// What a repository version needs before it installs: asks the repository for the resolved
// tree, checks the launcher version against every `tempest` requirement in it (throws when the
// launcher is too old), and installs the resolved versions not installed yet, leaves first.
// Returns false when the user cancelled the install altogether.
export async function installDependenciesFor(
	gamePath: string,
	modId: string,
	version: string,
	displayName: string,
	mode: DependencyMode,
): Promise<boolean> {
	if (mode === "skip") return true;
	const appVersion = await getAppVersion();
	let resolution;
	try {
		resolution = await getRepoDependencies(modId, version, patchOf(gamePath), appVersion);
	} catch (error: unknown) {
		// The mod still installs; the CLI reports what it misses afterwards.
		console.warn(`dependencies of ${modId} ${version} could not be resolved`, error);
		return true;
	}
	const tooOld = unmetTempest(
		resolution.tempest.map((t) => t.requirement),
		appVersion,
	);
	if (tooOld.length > 0) {
		throw new Error(
			m.deps_needs_tempest({ requirement: tooOld.join(", "), version: appVersion }),
		);
	}
	const installed = await installedMods(gamePath);
	const unresolved = resolution.items.filter(
		(item) => !item.resolved && !covered(item, installed),
	);
	const needed: DependencyToInstall[] = resolution.items
		.filter((item) => item.resolved && !covered(item, installed))
		.map((item) => ({
			id: item.id,
			name: item.name,
			version: item.resolved!.version,
			sha256: item.resolved!.sha256,
			requiredBy: item.requiredBy,
		}))
		// Breadth first from the mod: reversed, a dependency's own dependencies land first.
		.reverse();
	for (const item of unresolved) {
		addToast({
			title: displayName,
			message: m.deps_unresolved({
				id: item.id,
				requirement: item.requirements.map(describeRequirement).join(", "),
			}),
			tone: "warning",
		});
	}
	if (needed.length === 0) return true;
	if (mode === "ask") {
		const choice = await confirmDependencies(displayName, needed);
		if (choice === "cancel") return false;
		if (choice === "skip") return true;
	}
	for (const dep of needed) {
		const outcome = await installFromRepo(gamePath, dep.id, dep.version, dep.name, {
			replace: true,
			sha256: dep.sha256,
			dependencies: "skip",
		});
		if (outcome.status !== "installed") return false;
	}
	return true;
}

// Before a local package installs: what its manifest lists that the instance lacks, looked up
// on the repository (newest approved version that fits the requirement and the instance's
// patch) and offered in the dialog. "Install" fetches them first, each resolving its own
// dependencies without asking again; "skip" installs the mod alone. Returns false when the user
// cancelled, throws when the package needs a newer launcher than this one.
export async function installDependenciesForPackage(
	gamePath: string,
	displayName: string,
	entries: string[],
	mode: DependencyMode,
): Promise<boolean> {
	if (mode === "skip" || entries.length === 0) return true;
	const deps = entries.map(parseDependency).filter((d): d is Dependency => d !== null);
	const needsTempest = tempestRequirement(deps);
	if (needsTempest !== undefined) {
		const appVersion = await getAppVersion();
		if (!satisfies(appVersion, needsTempest)) {
			throw new Error(
				m.deps_needs_tempest({ requirement: needsTempest, version: appVersion }),
			);
		}
	}
	const installed = await installedMods(gamePath);
	const missing = modDependencies(deps).filter((dep) => {
		const have = installed.find((mod) => mod.Id.toLowerCase() === dep.id && mod.Enabled);
		return !have || !satisfies(have.Version, dep.requirement);
	});
	if (missing.length === 0) return true;
	const needed = await lookUp(gamePath, displayName, missing);
	if (needed.length === 0) return true;
	if (mode === "ask") {
		const choice = await confirmDependencies(displayName, needed);
		if (choice === "cancel") return false;
		if (choice === "skip") return true;
	}
	return installAll(gamePath, needed);
}

// After a local package installed with dependencies missing (its manifest could not be read
// beforehand): offers to fetch each from the repository.
export async function offerMissingDependencies(
	gamePath: string,
	displayName: string,
	missing: Dependency[],
): Promise<void> {
	const needed = await lookUp(gamePath, displayName, missing);
	if (needed.length === 0) return;
	const choice = await confirmDependencies(displayName, needed);
	if (choice !== "install") return;
	await installAll(gamePath, needed);
}

// Installs the listed versions in order; stops at the first failure.
async function installAll(gamePath: string, needed: DependencyToInstall[]): Promise<boolean> {
	for (const dep of needed) {
		const outcome = await installFromRepo(gamePath, dep.id, dep.version, dep.name, {
			replace: true,
			sha256: dep.sha256,
			dependencies: "auto",
		});
		if (outcome.status !== "installed") return false;
	}
	return true;
}

// The newest approved repository version of each missing dependency that fits its requirement
// and the instance's patch; one the repository cannot provide gets a warning toast instead.
async function lookUp(
	gamePath: string,
	displayName: string,
	missing: Dependency[],
): Promise<DependencyToInstall[]> {
	const patch = patchOf(gamePath);
	const needed: DependencyToInstall[] = [];
	for (const dep of missing) {
		const versions = await getRepoVersions(dep.id).catch(() => []);
		const pick = versions
			.filter(
				(v) =>
					v.status === "approved" &&
					isCompatible(v.compat, patch) &&
					satisfies(v.version, dep.requirement),
			)
			.sort((a, b) => compareSemver(b.version, a.version))[0];
		if (pick) {
			needed.push({
				id: dep.id,
				name: dep.id,
				version: pick.version,
				sha256: pick.sha256,
				requiredBy: [displayName],
			});
		} else {
			addToast({
				title: displayName,
				message: m.deps_unresolved({
					id: dep.id,
					requirement: describeRequirement(dep.requirement),
				}),
				tone: "warning",
			});
		}
	}
	return needed;
}
