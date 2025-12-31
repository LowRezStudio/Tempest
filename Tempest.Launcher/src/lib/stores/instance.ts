import type { Instance } from "$lib/types/instance";
import { persistentAtom, persistentMap } from "@nanostores/persistent";
import { jsonSerializer } from "./common";
import { computed } from "nanostores";

export const lastLaunchedInstanceId = persistentAtom<string>("lastLaunchedInstanceId", undefined);
export const instanceMap = persistentMap<Record<string, Instance>>(
	"instances:",
	{},
	jsonSerializer,
);

export const lastLaunchedInstance = computed([instanceMap, lastLaunchedInstanceId], (instanceMap, id) =>
	id ? instanceMap[id] : undefined,
);

export const addInstance = (instance: Instance) => instanceMap.setKey(instance.id, instance);

export const updateInstance = (id: string, updates: Partial<Instance>) => {
	const current = instanceMap.get()[id];
	if (current) {
		instanceMap.setKey(id, { ...current, ...updates });
	}
};

export const removeInstance = (id: string) => instanceMap.setKey(id, undefined);
