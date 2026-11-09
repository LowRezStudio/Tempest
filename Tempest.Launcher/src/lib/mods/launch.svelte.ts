import { SvelteSet } from "svelte/reactivity";
import { installQueue } from "$lib/mods/installQueue.svelte";
import {
	activeModCommands,
	modOperationFailures,
	pendingInstalls,
} from "$lib/mods/pendingInstalls.svelte";
import { m } from "$lib/paraglide/messages";
import type { Instance } from "$lib/types/instance";

const scheduled = new SvelteSet<string>();
const requests = new Map<string, { controller: AbortController; promise: Promise<void> }>();

export const isLaunchScheduled = (instanceId: string) => scheduled.has(instanceId);

export const hasPendingModOperations = (gamePath: string) =>
	(activeModCommands.get(gamePath) ?? 0) > 0 ||
	pendingInstalls.value.some((item) => item.gamePath === gamePath) ||
	installQueue.value.some((item) => item.gamePath === gamePath);

export function cancelScheduledLaunch(instanceId: string): boolean {
	if (!scheduled.has(instanceId)) return false;
	requests.get(instanceId)?.controller.abort();
	return true;
}

function waitForModOperations(
	gamePath: string,
	signal: AbortSignal,
	failures: number,
): Promise<void> {
	return new Promise((resolve, reject) => {
		let settled = false;
		const finish = (error?: Error) => {
			if (settled) return;
			settled = true;
			signal.removeEventListener("abort", onAbort);
			dispose();
			if (error) reject(error);
			else resolve();
		};
		const onAbort = () => finish(new DOMException("Launch cancelled", "AbortError"));
		const check = () => {
			if (signal.aborted) onAbort();
			else if ((modOperationFailures.get(gamePath) ?? 0) !== failures) {
				finish(new Error(m.launch_mod_operations_failed()));
			} else if (!hasPendingModOperations(gamePath)) finish();
		};
		signal.addEventListener("abort", onAbort, { once: true });
		const dispose = $effect.root(() => {
			$effect(check);
		});
		check();
	});
}

export function launchAfterModOperations(
	instance: Instance,
	start: (wait: () => Promise<boolean>) => Promise<void>,
): Promise<void> {
	const existing = requests.get(instance.id);
	if (existing) return existing.promise;
	const controller = new AbortController();
	const failures = modOperationFailures.get(instance.path) ?? 0;
	if (hasPendingModOperations(instance.path)) scheduled.add(instance.id);
	const wait = async () => {
		const waiting = hasPendingModOperations(instance.path);
		if (waiting) scheduled.add(instance.id);
		try {
			await waitForModOperations(instance.path, controller.signal, failures);
		} finally {
			scheduled.delete(instance.id);
		}
		return waiting;
	};
	const promise = Promise.resolve()
		.then(async () => {
			await wait();
			await start(wait);
		})
		.catch((error: unknown) => {
			if (!controller.signal.aborted) throw error;
		})
		.finally(() => {
			scheduled.delete(instance.id);
			requests.delete(instance.id);
		});
	requests.set(instance.id, { controller, promise });
	return promise;
}
