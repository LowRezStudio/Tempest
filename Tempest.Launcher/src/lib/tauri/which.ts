import { invoke } from "@tauri-apps/api/core";

export const which = (name: string): Promise<string | null> => invoke("which", { name });
