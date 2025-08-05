import { PersistedState } from "runed";

export type Build = {
	id: string;
	label: string;
	path: string;
};

export const builds = new PersistedState<Build[]>("builds", []);
export const addBuild = (build: Omit<Build, "id">) =>
	builds.current = [{ id: crypto.randomUUID(), ...build }, ...builds.current];
export const removeBuild = (id: string) => builds.current = builds.current.filter(b => b.id != id);
