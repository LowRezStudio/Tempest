// Typed registration of `ipcMain.handle` channels against the IPC contract.
// Compile error on unknown channels or mismatched arg/result types.
import { app, ipcMain, type IpcMainInvokeEvent } from "electron";
import type { IpcContract } from "./contract.js";

export type IpcChannel = Extract<keyof IpcContract, string>;

export type IpcHandler<K extends IpcChannel> = (
	event: IpcMainInvokeEvent,
	args: IpcContract[K]["args"],
) => IpcContract[K]["result"] | Promise<IpcContract[K]["result"]>;

// The preload bridge forwards every channel, so untrusted senders (sub-frames, a
// navigated-away page) are refused here rather than in each handler.
export function isTrustedSender(event: IpcMainInvokeEvent): boolean {
	const frame = event.senderFrame;
	if (!frame || frame.parent !== null) return false;
	try {
		const url = new URL(frame.url);
		return app.isPackaged ? url.protocol === "app:" : url.origin === "http://localhost:1420";
	} catch {
		return false;
	}
}

export function handle<K extends IpcChannel>(channel: K, handler: IpcHandler<K>): void {
	ipcMain.handle(channel, (event, args: IpcContract[K]["args"]) => {
		if (!isTrustedSender(event)) throw new Error(`IPC ${channel} refused: untrusted sender`);
		return handler(event, args);
	});
}
