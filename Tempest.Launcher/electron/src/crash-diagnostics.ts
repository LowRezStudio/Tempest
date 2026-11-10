import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { redactLog } from "./crash-logs.js";

export type DiagnosticFile = { name: string; text: string };
export type DiagnosticResult = { files: DiagnosticFile[]; warnings: string[] };
const MAX_FILE_BYTES = 2 * 1024 * 1024;
const MAX_DEV_BYTES = 8 * 1024 * 1024;
const MAX_DEV_FILES = 500;
const SOURCE = /\.(?:lua|toml|json|ini|cfg|conf|js|ts|cs|c|cpp|h|hpp|txt)$/i;

const errorText = (error: unknown) => (error instanceof Error ? error.message : String(error));
const missing = (error: unknown) =>
	error instanceof Error && "code" in error && error.code === "ENOENT";
const isRecord = (value: unknown): value is Record<string, unknown> =>
	typeof value === "object" && value !== null;

async function readLimited(file: string, limit: number): Promise<Buffer> {
	const opened = await fs.open(file, "r");
	try {
		const info = await opened.stat();
		if (info.size > limit) throw new Error(`Diagnostic file exceeds its ${limit}-byte limit.`);
		const buffer = Buffer.alloc(info.size + 1);
		let length = 0;
		while (length < buffer.length) {
			const { bytesRead } = await opened.read(buffer, length, buffer.length - length, null);
			if (!bytesRead) break;
			length += bytesRead;
		}
		if (length > limit) throw new Error(`Diagnostic file exceeds its ${limit}-byte limit.`);
		if (length > info.size)
			throw new Error("Diagnostic file grew while it was being collected.");
		return buffer.subarray(0, length);
	} finally {
		await opened.close();
	}
}

export function redactDiagnostics(text: string): string {
	return redactLog(text)
		.replaceAll(
			/-----BEGIN [^-]*PRIVATE KEY-----[\s\S]*?-----END [^-]*PRIVATE KEY-----/g,
			"<private key removed>",
		)
		.replaceAll(
			/^(\s*(?:Machine Id|DxDiag Machine Id|Serial Number|Monitor Serial Number)\s*:\s*).*$/gim,
			"$1<removed>",
		);
}

export async function collectDevMods(
	directory: string,
): Promise<DiagnosticResult & { mods: string[] }> {
	const result: DiagnosticResult & { mods: string[] } = { files: [], warnings: [], mods: [] };
	let bytes = 0;
	let visited = 0;
	const warn = (message: string) => result.warnings.push(redactDiagnostics(message));
	let roots;
	try {
		const root = await fs.lstat(directory);
		if (!root.isDirectory() || root.isSymbolicLink()) {
			warn("Dev-mod directory is not a regular directory; it was not followed.");
			return result;
		}
		roots = await fs.readdir(directory, { withFileTypes: true });
	} catch (error) {
		if (!missing(error)) warn(`Could not read dev mods: ${errorText(error)}`);
		return result;
	}

	const walk = async (folder: string, relative: string): Promise<void> => {
		const resolved = await fs.realpath(folder);
		const root = await fs.realpath(directory);
		const within = path.relative(root, resolved);
		if (within.startsWith("..") || path.isAbsolute(within))
			throw new Error("Dev-mod directory escapes its root.");
		const entries = await fs.readdir(folder, { withFileTypes: true });
		for (const entry of entries.sort((a, b) => a.name.localeCompare(b.name))) {
			if (entry.name.startsWith(".")) continue;
			if (++visited > MAX_DEV_FILES) {
				if (visited === MAX_DEV_FILES + 1)
					warn(`Dev-mod collection stopped at ${MAX_DEV_FILES} entries.`);
				return;
			}
			const file = path.join(folder, entry.name);
			const name = `${relative}/${entry.name}`;
			if (entry.isSymbolicLink()) {
				warn(`Skipped dev-mod link: ${name}`);
			} else if (entry.isDirectory()) {
				await walk(file, name);
			} else if (entry.isFile()) {
				const info = await fs.stat(file);
				const reason = /(?:credential|secret|password|private[_-]?key)/i.test(entry.name)
					? "Potentially sensitive file."
					: !SOURCE.test(entry.name)
						? "Not a source/config text file; binaries are not attached."
						: info.size > MAX_FILE_BYTES || bytes + info.size > MAX_DEV_BYTES
							? "Source/config attachment size limit exceeded."
							: undefined;
				if (reason) {
					if (SOURCE.test(entry.name)) warn(`Skipped ${name}: ${reason}`);
					continue;
				}
				const realFile = await fs.realpath(file);
				const within = path.relative(root, realFile);
				if (within.startsWith("..") || path.isAbsolute(within))
					throw new Error("Dev-mod file escapes its root.");
				const data = await readLimited(realFile, MAX_FILE_BYTES);
				if (data.length > MAX_FILE_BYTES || bytes + data.length > MAX_DEV_BYTES) {
					warn(`Skipped ${name}: file grew beyond the source/config attachment limit.`);
					continue;
				}
				if (data.includes(0)) {
					warn(`Skipped binary content in dev-mod text file: ${name}`);
					continue;
				}
				bytes += data.length;
				result.files.push({
					name: `dev-mods/${name}`,
					text: redactDiagnostics(data.toString("utf8")),
				});
			}
		}
	};

	for (const root of roots.sort((a, b) => a.name.localeCompare(b.name))) {
		if (!root.isDirectory() || root.name.startsWith(".") || root.name === "types") continue;
		try {
			const folder = path.join(directory, root.name);
			const children = await fs.readdir(folder, { withFileTypes: true });
			const isMod = children.some(
				(entry) =>
					(entry.name === "scripts" && entry.isDirectory()) ||
					(entry.isFile() &&
						entry.name.endsWith(".lua") &&
						!entry.name.endsWith(".d.lua")),
			);
			if (isMod) {
				result.mods.push(redactDiagnostics(root.name));
				await walk(folder, root.name);
			}
		} catch (error) {
			warn(`Could not collect dev mod ${root.name}: ${errorText(error)}`);
		}
	}
	return result;
}

export async function collectModuleSnapshot(
	file: string | undefined,
	expectedDirectory = path.join(os.tmpdir(), "TempestCrashDiagnostics"),
): Promise<DiagnosticResult> {
	const result: DiagnosticResult = { files: [], warnings: [] };
	if (!file) {
		result.warnings.push("Loaded-module inventory is unavailable for this launch.");
		return result;
	}
	// Only consume the per-session files the CLI creates, not arbitrary renderer-supplied paths.
	if (
		path.relative(path.resolve(expectedDirectory), path.dirname(path.resolve(file))) !== "" ||
		!/^[0-9a-f]{32}\.json$/i.test(path.basename(file))
	) {
		result.warnings.push("The module snapshot path is not a CLI diagnostic snapshot.");
		return result;
	}
	try {
		const info = await fs.lstat(file);
		if (!info.isFile() || info.isSymbolicLink() || info.size > MAX_FILE_BYTES) {
			throw new Error("The module snapshot is not a regular file within the size limit.");
		}
		const text = (await readLimited(file, MAX_FILE_BYTES)).toString("utf8");
		const snapshot: unknown = JSON.parse(text.replace(/^\uFEFF/, ""));
		if (
			!isRecord(snapshot) ||
			!Array.isArray(snapshot.modules) ||
			!Array.isArray(snapshot.errors)
		) {
			throw new Error("Invalid module snapshot format.");
		}
		if (snapshot.complete !== true)
			result.warnings.push("Loaded-module collection was incomplete.");
		for (const error of snapshot.errors) {
			if (typeof error === "string") result.warnings.push(redactDiagnostics(error));
		}
		if (!snapshot.modules.length)
			result.warnings.push("No loaded modules could be observed for this session.");
		const modules = snapshot.modules.map((module: unknown) => {
			if (!isRecord(module) || typeof module.path !== "string" || !module.path.trim()) {
				throw new Error("Invalid module path in snapshot.");
			}
			const sha256 =
				typeof module.sha256 === "string" && /^[0-9a-f]{64}$/i.test(module.sha256)
					? module.sha256.toLowerCase()
					: null;
			const error = sha256
				? undefined
				: typeof module.error === "string" && module.error
					? module.error
					: "SHA-256 could not be collected.";
			return {
				path: redactDiagnostics(module.path),
				sha256,
				...(error ? { error: redactDiagnostics(error) } : {}),
			};
		});
		const failed = modules.filter((module) => module.error).length;
		if (failed)
			result.warnings.push(
				`SHA-256 could not be collected for ${failed} module(s); see loaded-modules.json.`,
			);
		result.files.push({
			name: "loaded-modules.json",
			text: JSON.stringify(
				{
					modules,
					errors: result.warnings,
					complete: snapshot.complete === true,
				},
				null,
				2,
			),
		});
		await fs.unlink(file);
	} catch (error) {
		result.warnings.push(
			redactDiagnostics(`Could not collect loaded modules: ${errorText(error)}`),
		);
	}
	return result;
}

export function collectSystemInfo(): string {
	const cpus = os.cpus();
	const megabytes = (bytes: number) => Math.round(bytes / (1024 * 1024));
	return [
		"System Information",
		"------------------",
		`Operating System: ${os.type()} ${os.release()} (${os.version()})`,
		`Architecture: ${os.arch()}`,
		`Processor: ${cpus[0]?.model || "(unavailable)"}`,
		`Logical processors: ${cpus.length}`,
		`Memory: ${megabytes(os.totalmem())} MB RAM`,
		`Available memory: ${megabytes(os.freemem())} MB`,
		"",
	].join("\n");
}
