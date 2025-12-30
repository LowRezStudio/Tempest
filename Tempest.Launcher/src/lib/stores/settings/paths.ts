import { persistentAtom } from "@nanostores/persistent";
import { path } from "@tauri-apps/api";
import { homeDir } from "@tauri-apps/api/path";
import { platform } from "@tauri-apps/plugin-os";

const getDefaultInstancePath = async () => {
	const rootDir = platform() == "windows" ? "C:" : await homeDir();

	return await path.join(rootDir, "Games", "Tempest");
};

export const defaultInstancePath = persistentAtom<string | undefined>(
	"defaultInstancePath",
	undefined,
);
