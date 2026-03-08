import type { Instance } from "./instance";
import type { Child, Command } from "@tauri-apps/plugin-shell";

export type Process = {
	status: "on" | "setup" | "off";
	child: Child;
	command: Command<string>;
	instance: Instance;
};
