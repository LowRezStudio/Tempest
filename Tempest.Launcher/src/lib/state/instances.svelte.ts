import { path } from "@tauri-apps/api";
import { homeDir } from "@tauri-apps/api/path";
import { platform } from "@tauri-apps/plugin-os";
import { PersistedState } from "runed";

export type Instance = {
	id: string;
	label: string;
	version?: string;
	path: string;
	state:
		| {
			type: "unprepared"
			status: "downloading" | "paused";
			percentage: number;
		}
		| {
			type: "prepared"
		};
};

const getDefaultInstancePath = async () => {
	const rootDir = platform() == "windows" ? "C:" : await homeDir();

	return await path.join(rootDir, "Games", "Tempest");
};

export const instances = new PersistedState<Instance[]>("instances", []);
export const addInstance = (build: Omit<Instance, "id">) =>
	instances.current.push({ id: crypto.randomUUID(), ...build });
export const getInstance = (id: string) => instances.current.find(i => i.id == id);
export const removeInstance = (id: string) => instances.current = instances.current.filter(i => i.id != id);

export const defaultInstancePath = new PersistedState<string>("defaultInstancePath", "");

if (defaultInstancePath.current == "") {
	getDefaultInstancePath().then((result) => {
		defaultInstancePath.current = result;
	});
}

// little migration for use in development
// to-do: remove for release
for (const instance of instances.current) {
	if (!Object.hasOwn(instance, "state")) {
		instance.state = { type: "prepared" };
	}
}