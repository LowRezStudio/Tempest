import type { Child, Command } from "@tauri-apps/plugin-shell";
import type { Instance } from "./instance";

export type Process = {
	status: "on" | "setup" | "off";
	child: Child;
	command: Command<string>;
	instance: Instance;
};
