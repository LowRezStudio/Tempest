import type { Process } from "$lib/types/process";
import { atom } from "nanostores";

export const processesList = atom<Process[]>([]);
