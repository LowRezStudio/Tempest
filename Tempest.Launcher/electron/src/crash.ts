// Game crash reports: finds out whether a session crashed, stages its redacted logs and sends
// them to the crash channel when the player asks. Nothing leaves the machine without that.
import fs from "node:fs/promises";
import path from "node:path";
import { BrowserWindow, dialog, net } from "electron";
import { collectDevMods, collectModuleSnapshot, collectSystemInfo } from "./crash-diagnostics.js";
import { crashInstallationId } from "./crash-identity.js";
import { crashSummary, findCrash, redactLog, type SessionLog } from "./crash-logs.js";
import { handle } from "./ipc/handle.js";
import { buildZip } from "./zip.js";
import type { CrashDetails, CrashSummary } from "./ipc/contract.js";

// The relay (repository tempest-crash-reports) keeps the Discord webhook out of the app, where
// anyone could read it. The request is a Discord webhook call, so the override can also be a webhook.
const REPORT_URL =
	process.env.TEMPEST_CRASH_REPORT_URL ?? "https://crash-reports.lowrezstudio.com/report";

const MAX_LOG_BYTES = 8 * 1024 * 1024;
const MAX_DUMP_BYTES = 6 * 1024 * 1024;
// Discord refuses attachments over 10 MiB on a server without boosts.
const MAX_REPORT_BYTES = 9 * 1024 * 1024;
// File times are not exact, and the log is created just after the launch.
const CLOCK_SLACK_MS = 2000;

type StagedCrash = CrashSummary & {
	logs: { name: string; text: string }[];
	dumpPath: string | null;
	devMods: string[];
};

let staged: StagedCrash | null = null;

async function filesSince(folder: string, pattern: RegExp, since: number) {
	const names = await fs.readdir(folder).catch(() => [] as string[]);
	const files = await Promise.all(
		names
			.filter((name) => pattern.test(name))
			.map(async (name) => {
				const file = path.join(folder, name);
				const stat = await fs.stat(file).catch(() => null);
				return stat ? { name, file, size: stat.size, modified: stat.mtimeMs } : null;
			}),
	);
	return files
		.flatMap((f) => (f && f.modified >= since - CLOCK_SLACK_MS ? [f] : []))
		.sort((a, b) => b.modified - a.modified);
}

async function readTail(file: string, size: number): Promise<string> {
	const opened = await fs.open(file, "r");
	try {
		const length = Math.min(size, MAX_LOG_BYTES);
		const buffer = Buffer.alloc(length);
		await opened.read(buffer, 0, length, size - length);
		return buffer.toString("utf8");
	} finally {
		await opened.close();
	}
}

handle("crash:inspect", async (_event, { folders, startedAt, exitCode }) => {
	const logFiles = await filesSince(folders.logs, /^Launch(_\d+)?\.log$/i, startedAt);
	const logs: SessionLog[] = await Promise.all(
		logFiles.map(async ({ name, file, size }) => ({ name, text: await readTail(file, size) })),
	);
	const crash = findCrash(logs, exitCode);
	const modules = await collectModuleSnapshot(folders.modules);
	if (!crash) return null;
	const included: { name: string; text: string }[] = [];
	const launch = crash.log ? redactLog(crash.log.text) : "";
	if (crash.log) included.push({ name: "Launch.log", text: launch });
	// The scripting runtime's vectored exception handler is the only source of a stack for hard
	// faults the engine never got to log, so its log feeds the summary, not just the archive.
	const [scripting] = await filesSince(folders.temp, /^tempest-scripting\.log$/i, startedAt);
	const scriptingText = scripting
		? redactLog(await readTail(scripting.file, scripting.size))
		: "";
	if (scripting) included.push({ name: "tempest-scripting.log", text: scriptingText });
	const [dump] = await filesSince(folders.logs, /^unreal-.*\.dmp$/i, startedAt);
	const dumpPath = dump && dump.size <= MAX_DUMP_BYTES ? dump.file : null;
	const dev = folders.devMods
		? await collectDevMods(folders.devMods)
		: {
				files: [],
				mods: [],
				warnings: [
					"Dev-mod source/config collection is unavailable for this launch. Rebuild the CLI and relaunch the game.",
				],
			};
	included.push(...dev.files, ...modules.files);
	const diagnosticWarnings = [...dev.warnings, ...modules.warnings];
	if (diagnosticWarnings.length)
		included.push({ name: "diagnostics-warnings.txt", text: diagnosticWarnings.join("\n") });

	const summary: CrashSummary = {
		id: crypto.randomUUID(),
		exitCode,
		...crashSummary(launch, scriptingText),
		files: included.map(({ name, text }) => ({ name, size: Buffer.byteLength(text) })),
		dumpSize: dumpPath ? dump.size : null,
		diagnosticWarnings,
	};
	staged = { ...summary, logs: included, dumpPath, devMods: dev.mods };
	return summary;
});

const exitCodeText = (code: number | null) =>
	code === null ? "none" : `0x${(code >>> 0).toString(16).toUpperCase().padStart(8, "0")}`;

function reportText(crash: StagedCrash, details: CrashDetails): string {
	return [
		`Game: ${details.game}`,
		`Exit code: ${exitCodeText(crash.exitCode)}`,
		`Launcher: ${details.launcher}`,
		`OS: ${details.os}`,
		`Discord: ${details.discord || "(not provided)"}`,
		`Note: ${details.note || "(none)"}`,
		"",
		"Enabled mods:",
		...(details.mods.length > 0 ? details.mods : ["(none)"]),
		"",
		"Dev mods (local source/config attached):",
		...(crash.devMods.length > 0 ? crash.devMods : ["(none found)"]),
		"",
		"Diagnostic warnings:",
		...(crash.diagnosticWarnings.length > 0 ? crash.diagnosticWarnings : ["(none)"]),
		"",
		"Crash:",
		...crash.message,
		...crash.stack,
		"",
	].join("\n");
}

async function buildReport(
	id: string,
	details: CrashDetails,
	includeDump: boolean,
	includeSystemInfo: boolean,
): Promise<{ crash: StagedCrash; zip: Uint8Array }> {
	if (staged?.id !== id) throw new Error("This crash's logs are no longer available.");
	const crash = staged;
	const entries: { path: string; data: Uint8Array | string }[] = [
		{ path: "report.txt", data: reportText(crash, details) },
		...crash.logs.map(({ name, text }) => ({ path: name, data: text })),
		{ path: "launcher.log", data: redactLog(details.launcherLog) },
	];
	if (includeDump && crash.dumpPath) {
		entries.push({ path: "crash.dmp", data: await fs.readFile(crash.dumpPath) });
	}
	if (includeSystemInfo)
		entries.push({ path: "system-information.txt", data: collectSystemInfo() });
	return { crash, zip: buildZip(entries) };
}

const clip = (text: string, length: number) =>
	text.length > length ? `${text.slice(0, length - 1)}…` : text;

const reportFileName = () =>
	`tempest-crash-${new Date().toISOString().slice(0, 19).replaceAll(/[-:]/g, "").replace("T", "-")}.zip`;

function discordPayload(crash: StagedCrash, details: CrashDetails) {
	const summary = [...crash.message, ...crash.stack].join("\n").replaceAll("```", "'''");
	return {
		username: "Tempest crash reports",
		allowed_mentions: { parse: [] },
		embeds: [
			{
				title: clip(`Crash: ${details.game}`, 200),
				color: 0xe05561,
				description: summary
					? `\`\`\`\n${clip(summary, 1800)}\n\`\`\``
					: "The game stopped without logging an error.",
				fields: [
					{ name: "Exit code", value: exitCodeText(crash.exitCode), inline: true },
					{ name: "Launcher", value: clip(details.launcher, 100), inline: true },
					{ name: "OS", value: clip(details.os, 100), inline: true },
					...(details.discord
						? [{ name: "Player's Discord", value: clip(details.discord, 100) }]
						: []),
					{
						name: "Enabled mods",
						value: clip(details.mods.join(", ") || "none", 1000),
					},
					...(crash.devMods.length
						? [{ name: "Dev mods", value: clip(crash.devMods.join(", "), 1000) }]
						: []),
					...(details.note
						? [{ name: "Player's note", value: clip(details.note, 1000) }]
						: []),
				],
			},
		],
	};
}

handle("crash:send", async (_event, { id, details, includeDump, includeSystemInfo }) => {
	const { crash, zip } = await buildReport(id, details, includeDump, includeSystemInfo);
	if (zip.length > MAX_REPORT_BYTES) return { status: "too-large" };

	const form = new FormData();
	form.set("installation_id", await crashInstallationId());
	form.set("payload_json", JSON.stringify(discordPayload(crash, details)));
	form.set(
		"files[0]",
		new Blob([Buffer.from(zip)], { type: "application/zip" }),
		reportFileName(),
	);
	try {
		const response = await net.fetch(REPORT_URL, { method: "POST", body: form });
		if (response.ok) return { status: "sent" };
		return response.status === 429
			? { status: "rate-limited" }
			: { status: "failed", error: `HTTP ${response.status}` };
	} catch (error) {
		return { status: "failed", error: error instanceof Error ? error.message : String(error) };
	}
});

handle("crash:save", async (event, { id, details, includeDump, includeSystemInfo }) => {
	const { zip } = await buildReport(id, details, includeDump, includeSystemInfo);
	const window = BrowserWindow.fromWebContents(event.sender);
	const options = {
		defaultPath: reportFileName(),
		filters: [{ name: "Zip archive", extensions: ["zip"] }],
	};
	const result = window
		? await dialog.showSaveDialog(window, options)
		: await dialog.showSaveDialog(options);
	if (result.canceled || !result.filePath) return null;
	await fs.writeFile(result.filePath, zip);
	return result.filePath;
});
