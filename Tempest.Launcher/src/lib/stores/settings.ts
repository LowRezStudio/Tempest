import { persistentAtom } from "@nanostores/persistent";
import { path } from "@tauri-apps/api";
import { homeDir } from "@tauri-apps/api/path";
import { platform } from "@tauri-apps/plugin-os";
import { onMount, task } from "nanostores";

export const username = persistentAtom<string>("username", "Player");

const getDefaultInstancePath = async () => {
	const rootDir = platform() == "windows" ? "C:" : await homeDir();

	return await path.join(rootDir, "Games", "Tempest");
};

export const defaultInstancePath = persistentAtom<string | undefined>(
	"defaultInstancePath",
	undefined,
);

onMount(defaultInstancePath, () => {
	if (!globalThis.window || defaultInstancePath.get()) return;

	task(async () => defaultInstancePath.set(await getDefaultInstancePath()));
});
