// Typed registration of `ipcMain.handle` channels against the IPC contract.
// Compile error on unknown channels or mismatched arg/result types.
import { app, ipcMain, type IpcMainInvokeEvent } from "electron";
import type { IpcContract } from "./contract.js";

export type IpcChannel = Extract<keyof IpcContract, string>;

export type IpcHandler<K extends IpcChannel> = (
	event: IpcMainInvokeEvent,
	args: IpcContract[K]["args"],
) => IpcContract[K]["result"] | Promise<IpcContract[K]["result"]>;

/**
 * Whether an invoke comes from the top frame of our own page. The preload bridge
 * forwards any channel, so the host is where untrusted senders are refused: a
 * sub-frame or a navigated-away page must never reach shell, opener or Defender.
 * @param event - The invoke event whose sender frame is checked.
 * @returns Whether the sender is our top-level page.
 */
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
