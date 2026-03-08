import { atom } from "nanostores";
import type { Process } from "$lib/types/process";

export const processesList = atom<Process[]>([]);
