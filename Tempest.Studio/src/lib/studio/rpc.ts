// Typed calls into `upk serve` through the host (`upk:call`).
import { invoke, listen } from "$lib/platform/api";
import type {
	CompileFailure,
	UpkEvent,
	UpkMethod,
	UpkMethods,
} from "../../../electron/src/ipc/contract";

export function rpc<M extends UpkMethod>(
	method: M,
	params: UpkMethods[M]["params"],
): Promise<UpkMethods[M]["result"]> {
	return invoke("upk:call", { method, params }) as Promise<UpkMethods[M]["result"]>;
}

export function onUpkEvent(handler: (event: UpkEvent) => void): () => void {
	return listen("upk:event", handler);
}

/** Object URL for bytes the host decoded from a preview response. */
export function blobUrl(bytes: Uint8Array, type: string): string {
	return URL.createObjectURL(new Blob([bytes as BlobPart], { type }));
}

/** The structured part of a `script.compile` rejection. */
export function compileFailure(error: unknown): CompileFailure {
	const message = error instanceof Error ? error.message : String(error);
	const i = message.indexOf("{");
	if (message.startsWith("compile failed:") && i >= 0) {
		try {
			return JSON.parse(message.slice(i)) as CompileFailure;
		} catch {
			// fall through
		}
	}
	return { message };
}
