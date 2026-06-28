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

export const addInstance = (instance: Instance) => instanceMap.setKey(instance.id, instance);

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
