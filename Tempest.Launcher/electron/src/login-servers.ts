// Login server registration fetches the game's discovery document and the
// MCTS identity key in the Electron host, where renderer CORS does not apply.
import { isIP } from "node:net";
import { net } from "electron";
import { handle } from "./ipc/handle.js";
import type { LoginServerDiscovery, LoginServerDiscoveryResult } from "./ipc/contract.js";

const DISCOVERY_PATH = "/Discovery/Paladins/PC/0.0.0000.0?sandbox=402";
const REQUEST_TIMEOUT_MS = 15_000;

function networkError(url: string, label: string, cause: unknown): string {
	const detail = cause instanceof Error ? cause.message : String(cause);
	const code = detail.match(/net::(ERR_[A-Z0-9_]+)/i)?.[1]?.toUpperCase();
	const { origin, hostname } = new URL(url);

	if (code === "ERR_CONNECTION_REFUSED") {
		return `Connection refused: ${origin} (${label}). Is the server running?`;
	}
	if (code === "ERR_NAME_NOT_RESOLVED") {
		return `Can't find ${hostname}. Check the server address.`;
	}
	if (code === "ERR_TIMED_OUT" || code === "ERR_CONNECTION_TIMED_OUT") {
		return `Connection timed out: ${origin} (${label}).`;
	}
	if (code?.startsWith("ERR_CERT_")) {
		return `HTTPS certificate rejected: ${hostname}.`;
	}
	if (code === "ERR_INTERNET_DISCONNECTED") {
		return "No network connection.";
	}
	return `Request failed: ${url} (${detail})`;
}

function normalizeBaseUrl(input: string): URL {
	const trimmed = input.trim();
	if (!trimmed) {
		throw new Error("Enter a server address.");
	}

	let url: URL;
	try {
		url = new URL(/^https?:\/\//i.test(trimmed) ? trimmed : `http://${trimmed}`);
	} catch {
		throw new Error("Enter a valid address, such as http://localhost:8080.");
	}

	if (
		(url.protocol !== "http:" && url.protocol !== "https:") ||
		!url.hostname ||
		url.username ||
		url.password ||
		url.pathname !== "/" ||
		url.search ||
		url.hash
	) {
		throw new Error("Enter an HTTP(S) base address without a path or credentials.");
	}

	return url;
}

function normalizeAddress(value: unknown): string {
	if (typeof value !== "string") {
		throw new TypeError("The discovery document has an invalid address.");
	}
	const rawAddress = value.trim();
	const address =
		rawAddress.startsWith("[") && rawAddress.endsWith("]")
			? rawAddress.slice(1, -1)
			: rawAddress;
	const hostname =
		/^(?=.{1,253}$)(?:[a-z\d](?:[a-z\d-]{0,61}[a-z\d])?)(?:\.(?:[a-z\d](?:[a-z\d-]{0,61}[a-z\d])?))*\.?$/i;
	const numericAddress = /^[\d.]+$/.test(address);
	if (!address || (isIP(address) === 0 && (!hostname.test(address) || numericAddress))) {
		throw new Error("Invalid server address in discovery response.");
	}
	return address;
}

function normalizePublicKey(value: string): string {
	const key = value.trim().toUpperCase();
	if (!/^[0-9A-F]{512}$/.test(key)) {
		throw new Error("Invalid server public key (expected 512 hex characters).");
	}
	return key;
}

async function getText(url: string, label: string): Promise<string> {
	let response: Response;
	try {
		response = await net.fetch(url, {
			signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS),
			redirect: "error",
		});
	} catch (error) {
		throw new Error(networkError(url, label, error));
	}
	if (!response.ok) {
		throw new Error(`Request failed: ${url} (HTTP ${response.status} ${response.statusText}).`);
	}
	return response.text();
}

async function discoverLoginServer(input: string): Promise<LoginServerDiscovery> {
	const baseUrl = normalizeBaseUrl(input).origin;
	const discoveryUrl = `${baseUrl}${DISCOVERY_PATH}`;
	const [documentText, identityText] = await Promise.all([
		getText(discoveryUrl, "discovery document"),
		getText(`${baseUrl}/identity/public-key`, "public key"),
	]);

	let document: unknown;
	try {
		document = JSON.parse(documentText);
	} catch {
		throw new Error(`Invalid JSON from ${discoveryUrl}.`);
	}
	if (!document || typeof document !== "object") {
		throw new Error(`Invalid discovery response from ${discoveryUrl}.`);
	}

	const record = document as { addresses?: unknown; [key: string]: unknown };
	if (!Array.isArray(record.addresses) || record.addresses.length === 0) {
		throw new Error(`No server addresses in ${discoveryUrl}.`);
	}
	const addresses = record.addresses.map(normalizeAddress);
	const discoveryKey =
		typeof record["public-key"] === "string" ? normalizePublicKey(record["public-key"]) : null;
	const publicKey = normalizePublicKey(identityText);
	if (!discoveryKey) {
		throw new Error(`No public key in ${discoveryUrl}.`);
	}
	if (discoveryKey !== publicKey) {
		throw new Error("Public key mismatch between discovery and /identity/public-key.");
	}

	return { baseUrl, discoveryUrl, addresses, publicKey };
}

handle("login-server:discover", async (_event, { url }): Promise<LoginServerDiscoveryResult> => {
	try {
		return { ok: true, loginServer: await discoverLoginServer(url) };
	} catch (error) {
		return { ok: false, message: error instanceof Error ? error.message : String(error) };
	}
});
