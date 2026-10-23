// Finding Paladins installs runs entirely on the host (`platforms:find-paladins`): Steam
// libraries, Epic manifests, the registry and a sweep of the drives are filesystem work.
import { invoke } from "$lib/platform/api";
import type { FoundInstallation } from "../../../electron/src/ipc/contract";

export type { FoundInstallation, InstallationSource } from "../../../electron/src/ipc/contract";

/**
 * Every Paladins install the host can find on this machine.
 * @param folders - Folders Tempest downloads into, searched one level deep.
 * @param sweep - Also look through the drives, not only the known launchers.
 * @returns The installs, those from a launcher first.
 */
export function findPaladinsInstallations(
	folders: string[],
	sweep = true,
): Promise<FoundInstallation[]> {
	return invoke("platforms:find-paladins", { folders, sweep });
}

/**
 * The Steam copy of Paladins, without sweeping the drives.
 * @returns The installation path, or `null` when Steam does not contain Paladins.
 */
export async function findSteamPaladinsInstallation(): Promise<string | null> {
	const found = await findPaladinsInstallations([], false);
	return found.find((install) => install.source === "steam")?.path ?? null;
}
