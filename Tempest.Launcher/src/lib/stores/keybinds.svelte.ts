import { persistedJSON } from "./persisted.svelte";
import type { KeybindsConfig } from "$ipc/contract";

export const DEFAULT_KEYBINDS: KeybindsConfig = {
	mouseSensitivity: 25,
	fpsLimit: 144,
	bindings: [
		{ name: "W", command: "GBA_MoveForward" },
		{ name: "A", command: "GBA_StrafeLeft" },
		{ name: "S", command: "GBA_MoveBackward" },
		{ name: "D", command: "GBA_StrafeRight" },
		{ name: "E", command: "GBA_Ability_1" },
		{ name: "Q", command: "GBA_Ability_2" },
		{ name: "F", command: "GBA_Ability_3" },
		{ name: "R", command: "GBA_Reload" },
		{ name: "U", command: "GBA_UpgradeMenu" },
		{ name: "I", command: "GBA_CommonCards" },
		{ name: "K", command: "GBA_SkillInfo" },
		{ name: "B", command: "GBA_Emote" },
		{ name: "T", command: "GBA_Spray" },
		{ name: "G", command: "GBA_CosmeticWheelToggle" },
		{ name: "C", command: "GBA_Toggle3p" },
	],
};

export const keybinds = persistedJSON<KeybindsConfig>("keybinds.v5", DEFAULT_KEYBINDS);

export function resetKeybinds() {
	keybinds.value = structuredClone(DEFAULT_KEYBINDS);
}
