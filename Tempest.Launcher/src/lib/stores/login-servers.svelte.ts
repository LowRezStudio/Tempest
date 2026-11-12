import { persistedJSON } from "./persisted.svelte";
import type { LoginServer } from "$lib/types/login-server";

const STORAGE_KEY = "loginServers";
const LEGACY_STORAGE_KEY = "communityServers";

// Keep server records created by an earlier development build when renaming this feature.
if (typeof window !== "undefined") {
	const legacy = localStorage.getItem(LEGACY_STORAGE_KEY);
	if (legacy !== null) {
		if (localStorage.getItem(STORAGE_KEY) === null) {
			localStorage.setItem(STORAGE_KEY, legacy);
		}
		localStorage.removeItem(LEGACY_STORAGE_KEY);
	}
}

/** Login server details are shared by every instance and saved with launcher settings. */
export const loginServers = persistedJSON<LoginServer[]>(STORAGE_KEY, []);
