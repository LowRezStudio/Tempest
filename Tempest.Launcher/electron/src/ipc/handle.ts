// Typed registration of `ipcMain.handle` channels against the IPC contract.
// Compile error on unknown channels or mismatched arg/result types.
import { ipcMain, type IpcMainInvokeEvent } from "electron";
import type { IpcContract } from "./contract.js";

export type IpcChannel = Extract<keyof IpcContract, string>;

export type IpcHandler<K extends IpcChannel> = (
	event: IpcMainInvokeEvent,
	args: IpcContract[K]["args"],
) => IpcContract[K]["result"] | Promise<IpcContract[K]["result"]>;

export function handle<K extends IpcChannel>(channel: K, handler: IpcHandler<K>): void {
	ipcMain.handle(channel, (event, args: IpcContract[K]["args"]) => handler(event, args));
}
