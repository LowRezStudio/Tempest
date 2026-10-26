// Typed calls into `upk serve` through the host (`upk:call`).
import { invoke, listen } from "$lib/platform/api";
import { expandTables } from "./tables";
import type {
	CompileFailure,
	PackageTablesWire,
	UpkEvent,
	UpkFrame,
	UpkMethod,
	UpkMethods,
} from "../../../electron/src/ipc/contract";

const decoder = new TextDecoder();

/** Drop the binary payloads where the result holds `{"$bin": n}` markers. */
function placeBins(value: unknown, bins: Uint8Array[]): unknown {
	if (!value || typeof value !== "object") return value;
	if (Array.isArray(value)) {
		for (let i = 0; i < value.length; i++) value[i] = placeBins(value[i], bins);
		return value;
	}
	const record = value as Record<string, unknown>;
	if (typeof record.$bin === "number" && Object.keys(record).length === 1) {
		return bins[record.$bin] ?? new Uint8Array();
	}
	for (const key of Object.keys(record)) record[key] = placeBins(record[key], bins);
	return value;
}

/** Decode a framed reply: parse the JSON once here, in the renderer. */
function decodeFrame(frame: UpkFrame): unknown {
	const value = frame.json ? JSON.parse(decoder.decode(frame.json)) : frame.value;
	return frame.bins.length > 0 ? placeBins(value, frame.bins) : value;
}

export async function rpc<M extends UpkMethod>(
	method: M,
	params: UpkMethods[M]["params"],
): Promise<UpkMethods[M]["result"]> {
	const frame = (await invoke("upk:call", { method, params })) as UpkFrame;
	const value = decodeFrame(frame);
	if (method === "package.open" || method === "package.tables") {
		return expandTables(value as PackageTablesWire) as UpkMethods[M]["result"];
	}
	return value as UpkMethods[M]["result"];
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
