// What a game log says about how the session ended, and what must not leave the machine.

export type SessionLog = { name: string; text: string };

// The engine's error path. Every one of these logs also has a minidump next to it.
const FATAL = /StaticShutdownAfterError|=== Critical error: ===|appRequestExit\(1\)/;
const CLEAN_EXIT = /\] Exit: Exiting\./;

/**
 * @param logs - The logs written since the game started, newest first. Several windows of an
 * instance write Launch.log, Launch_2.log, so the newest need not be this window's.
 * @param exitCode - The game's exit code.
 * @returns The crashed session's log (undefined when it left none), or null when there was no crash.
 */
export function findCrash(
	logs: SessionLog[],
	exitCode: number | null,
): { log: SessionLog | undefined } | null {
	const fatal = logs.find((log) => FATAL.test(log.text));
	if (fatal) return { log: fatal };
	// A hard crash (access violation) never reaches the engine's handler: the log just stops.
	if (!exitCode) return null;
	const newest = logs[0];
	return newest && CLEAN_EXIT.test(newest.text) ? null : { log: newest };
}

const MAX_MESSAGE_LINES = 6;
const MAX_STACK_LINES = 8;

/**
 * @param text - A redacted log.
 * @returns The crash's message and the top of its stack, for showing without opening the log.
 */
export function crashSummary(text: string): { message: string[]; stack: string[] } {
	const lines = text.split(/\r?\n/);
	const message: string[] = [];
	const stack: string[] = [];
	let inCritical = false;
	for (const line of lines) {
		if (line.includes("=== Critical error: ===")) {
			inCritical = true;
			continue;
		}
		const frame = /^Address = (0x[0-9a-f]+).*\[in .*?([^\\/\]]+)\]/i.exec(line);
		if (frame) {
			if (stack.length < MAX_STACK_LINES) stack.push(`${frame[1]} ${frame[2]}`);
			continue;
		}
		const critical = /^\[[\d.]+\] Critical: (.*\S)/.exec(line);
		if (critical) message.push(critical[1]);
		else if (inCritical && /^\[[\d.]+\]/.test(line)) inCritical = false;
		else if (inCritical && line.trim()) message.push(line.trim());
	}
	return {
		message: [...new Set(message)].slice(0, MAX_MESSAGE_LINES).map((l) => l.slice(0, 300)),
		stack,
	};
}

const literal = (value: string) =>
	new RegExp(
		`(?<![\\p{L}\\p{N}_])${value.replaceAll(/[.*+?^${}()|[\]\\/]/g, "\\$&")}(?![\\p{L}\\p{N}_])`,
		"giu",
	);

/**
 * Removes what identifies the player or could sign in as them. Player names the game logs in
 * passing can stay, which the report dialog says.
 * @param text - A log.
 * @param identity - The OS user and computer names.
 * @returns The log, safe to send.
 */
export function redactLog(text: string, identity: { user: string; host: string }): string {
	let out = text
		// Vivox logs its requests whole, sign-in tokens and account names included.
		.replaceAll(/^.*\] VoiceChat:.*(?:\r?\n|$)/gm, "")
		.replaceAll(/\[json = .*$/gm, "[json = <removed>]")
		.replaceAll(/^(.*Init: (?:Computer|User): ).*$/gm, "$1<removed>")
		.replaceAll(/-(auth_\w+|epicusername|epicuserid)=("[^"]*"|\S+)/gi, "-$1=<removed>")
		.replaceAll(
			/([?&](?:name|[\w-]*(?:token|auth|key|password|secret|ticket)[\w-]*)=)[^&\s]+/gi,
			"$1<removed>",
		)
		.replaceAll(/\b(token|bearer)([ :=]+)[^\s,;"'<]+/gi, "$1$2<removed>")
		.replaceAll(/\beyJ[\w-]{8,}\.[\w-]+\.[\w-]*/g, "<removed>")
		.replaceAll(/\b7656119\d{10}\b/g, "<removed>")
		.replaceAll(/[\w.+-]+@[\w-]+\.[\w.-]+/g, "<removed>")
		.replaceAll(/(^|[\\/])(users|home)([\\/])[^\\/\s"'<>]+/gim, "$1$2$3<user>")
		.replaceAll(
			/\b(?:(?:25[0-5]|2[0-4]\d|1?\d?\d)\.){3}(?:25[0-5]|2[0-4]\d|1?\d?\d)\b/g,
			(ip) => (ip === "127.0.0.1" || ip === "0.0.0.0" ? ip : "<ip>"),
		);
	for (const value of [identity.user, identity.host]) {
		if (value.length >= 3) out = out.replace(literal(value), "<removed>");
	}
	return out;
}
