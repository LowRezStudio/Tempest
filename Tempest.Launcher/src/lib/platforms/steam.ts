// Steam / Paladins discovery runs entirely on the host
// (`platforms:find-steam-paladins`): scanning Steam roots, parsing
// libraryfolders.vdf and checking installations are filesystem work.
import { invoke } from "$lib/platform/api";

/** Finds the first Steam library containing a usable Paladins installation.
 * @returns The installation path, or `null` when Steam does not contain Paladins.
 */
export function findSteamPaladinsInstallation(): Promise<string | null> {
	return invoke("platforms:find-steam-paladins");
}
