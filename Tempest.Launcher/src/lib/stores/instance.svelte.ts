import { compareVersions } from "$lib/utils/version";
import { persistedState, persistedJSON, persistedMapState } from "./persisted.svelte";
import type { Instance } from "$lib/types/instance";

export const lastLaunchedInstanceId = persistedState<string | undefined>(
	"lastLaunchedInstanceId",
	undefined,
);
export const instanceMap = persistedMapState<Record<string, Instance>>("instances:", {});

export const instanceOrder = persistedJSON<string[]>("instanceOrder", []);

export const lastLaunchedInstance = {
	get value() {
		const id = lastLaunchedInstanceId.value;
		return id ? instanceMap.value[id] : undefined;
	},
};

/**
 * Reorder a filtered list by the persisted user order, appending leftovers.
 * @param items - The items to order.
 * @param order - The persisted id order to follow.
 * @returns The items in the given order, with unlisted items appended.
 */
const applyOrder = <T extends Instance>(items: T[], order: readonly string[]) => {
	const byId = new Map(items.map((i) => [i.id, i]));
	const sorted: T[] = [];
	for (const id of order) {
		const item = byId.get(id);
		if (item) {
			sorted.push(item);
			byId.delete(id);
		}
	}
	for (const item of byId.values()) sorted.push(item);
	return sorted;
};

/** Prepared instances sorted by the user's sidebar order. */
export const preparedInstances = {
	get value() {
		const all = Object.values(instanceMap.value).filter(
			(i): i is Instance => !!i && i.state?.type === "prepared",
		);
		return applyOrder(all, instanceOrder.value);
	},
};

export const addInstance = (instance: Instance) => instanceMap.setKey(instance.id, instance);

/** All instances ordered by the user's (sidebar/library) order, leftovers appended. */
export const orderedInstances = {
	get value() {
		const all = Object.values(instanceMap.value).filter((i): i is Instance => !!i);
		return applyOrder(all, instanceOrder.value);
	},
};

/** Persist the user's instance order sorted lowest→highest version. */
export const sortInstancesByVersion = () => {
	const sorted = [...orderedInstances.value].sort((a, b) =>
		compareVersions(a.version, b.version),
	);
	setInstanceOrder(sorted.map((i) => i.id));
};

export const updateInstance = (id: string, updates: Partial<Instance>) => {
	const current = instanceMap.get()[id];
	if (current) {
		instanceMap.setKey(id, { ...current, ...updates });
	}
};

export const removeInstance = (id: string) => instanceMap.setKey(id, undefined);

export const setInstanceOrder = (ids: string[]) => {
	instanceOrder.value = ids;
};
