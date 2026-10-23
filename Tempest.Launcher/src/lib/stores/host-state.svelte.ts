import { invoke } from "$lib/platform/api";

// The host hands its current state to the preload script, which runs before
// any page module. Reading it here keeps the initial value synchronous, so a
// reloaded page never flashes a default and no store has to await a fetch.
const snapshot = (typeof window === "undefined" ? {} : (window.__hostState ?? {})) as Record<
	string,
	unknown
>;

/**
 * A `$state` store mirrored into the Electron host. The host lives as long as
 * the app, so a renderer reload (Vite full reload, crash) keeps the value.
 * @param key - State slot name, e.g. `"launch:running"`.
 * @param fallback - Value used before anything was stored.
 * @returns A store with the same `.value` get/set API as the persisted stores.
 */
export function hostState<T>(key: string, fallback: T) {
	let value = $state<T>((snapshot[key] as T | undefined) ?? fallback);

	return {
		get value() {
			return value;
		},
		set value(next: T) {
			value = next;
			void invoke("state:set", { patch: { [key]: next } }).catch((error) =>
				console.warn(`[host-state] ${key} not saved:`, error),
			);
		},
		get() {
			return value;
		},
	};
}
