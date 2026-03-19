import { atom } from "nanostores";
import type { LobbyServerProcess, Process } from "$lib/types/process";

export const processesList = atom<Process[]>([]);

export const lobbyServerProcessesList = atom<LobbyServerProcess[]>([]);
