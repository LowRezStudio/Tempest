import { PersistedState } from "runed";

export type Instance = {
	id: string;
	label: string;
	version?: string;
	path: string;
};

export const instances = new PersistedState<Instance[]>("instances", []);
export const addInstance = (build: Omit<Instance, "id">) =>
	instances.current = [{ id: crypto.randomUUID(), ...build }, ...instances.current];
export const getInstance = (id: string) => instances.current.find(i => i.id == id);
export const removeInstance = (id: string) => instances.current = instances.current.filter(i => i.id != id);
