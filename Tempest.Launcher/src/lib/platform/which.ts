import { invoke } from "$lib/platform/api";

export const which = (name: string): Promise<string | null> => invoke("which", { name });
