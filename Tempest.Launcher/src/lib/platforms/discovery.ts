// Finding Paladins installs runs entirely on the host (`platforms:find-paladins`): Steam
// libraries, Epic manifests, the registry and a sweep of the drives are filesystem work.
import { invoke, listen } from "$lib/platform/api";
import type { FoundInstallation } from "$ipc/contract";

export type { FoundInstallation, InstallationSource } from "$ipc/contract";

/**
 * Every Paladins install the host can find on this machine.
 * @param folders - Folders Tempest downloads into, searched one level deep.
 * @param sweep - Also look through the drives, not only the known launchers.
 * @param onFound - Called with each install the moment it is found.
 * @returns The installs, those from a launcher first.
 */
export async function findPaladinsInstallations(
	folders: string[],
	sweep = true,
	onFound?: (install: FoundInstallation) => void,
): Promise<FoundInstallation[]> {
	if (!onFound) return invoke("platforms:find-paladins", { folders, sweep });
	const progressId = crypto.randomUUID();
	const unlisten = await listen("platforms:found-paladins", ({ id, install }) => {
		if (id === progressId) onFound(install);
	});
	try {
		return await invoke("platforms:find-paladins", { folders, sweep, progressId });
	} finally {
		unlisten();
	}
}
