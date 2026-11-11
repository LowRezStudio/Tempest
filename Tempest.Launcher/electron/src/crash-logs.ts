// What a game log says about how the session ended, and what must not leave the machine.

export type SessionLog = { name: string; text: string };

// appRequestExit(1) means a forced exit, not necessarily an error.
const FATAL = /StaticShutdownAfterError|=== Critical error: ===/;
const FATAL_CRITICAL =
	/^\[[\d.]+\] Critical: (?:appError called:|Assertion failed:|Rendering thread exception:|Fatal error!|Error reentered:)/m;
const FATAL_EXIT_CODES = new Set([
	0x80000002, // Datatype misalignment
	0x80000003, // Breakpoint
	0x80000004, // Single step
	0xc0000005, // Access violation
	0xc0000006, // In-page error
	0xc000001d, // Illegal instruction
	0xc000008c, // Array bounds exceeded
	0xc000008d, // Floating-point denormal operand
	0xc000008e, // Floating-point division by zero
	0xc000008f, // Floating-point inexact result
	0xc0000090, // Floating-point invalid operation
	0xc0000091, // Floating-point overflow
	0xc0000092, // Floating-point stack check
	0xc0000093, // Floating-point underflow
	0xc0000094, // Integer division by zero
	0xc0000095, // Integer overflow
	0xc0000096, // Privileged instruction
	0xc00000fd, // Stack overflow
	0xc00002b4, // Multiple floating-point faults
	0xc00002b5, // Multiple floating-point traps
	0xc0000374, // Heap corruption
	0xc0000409, // Fail-fast / stack buffer overrun
	0xc0000417, // Invalid C-runtime parameter
	0xc0000420, // Assertion failure
	0xc0000602, // Fail-fast exception
	0x40000015, // Fatal application exit
	0xe06d7363, // Unhandled C++ exception
	0xe0434352, // Unhandled CLR exception
]);

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
	const fatal = logs.find((log) => FATAL.test(log.text) || FATAL_CRITICAL.test(log.text));
	if (fatal) return { log: fatal };
	// Wrapper errors and normal forced shutdowns can also return non-zero codes.
	// A known fatal exception is evidence even if it happens during engine teardown.
	if (exitCode === null || !FATAL_EXIT_CODES.has(exitCode >>> 0)) return null;
	return { log: logs[0] };
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
		if (critical) {
			message.push(critical[1]);
			if (FATAL_CRITICAL.test(line)) inCritical = true;
		}
		else if (inCritical && /^\[[\d.]+\]/.test(line)) inCritical = false;
		else if (inCritical && line.trim()) message.push(line.trim());
	}
	return {
		message: [...new Set(message)].slice(0, MAX_MESSAGE_LINES).map((l) => l.slice(0, 300)),
		stack,
	};
}

const SECRET_ASSIGNMENT =
	/((?:["']?)\b[\w.-]{0,64}(?:token|auth|key|password|secret|ticket|credential)[\w.-]{0,64}(?:["']?\s*[:=]\s*))(?:"(?:\\.|[^"\\\r\n])*"|'[^'\r\n]*'|[^&\s,;#}\]]+)/gi;

/**
 * Removes credentials and identifiers while keeping player, account, and machine names.
 * @param text - A log.
 * @returns The log with recognizable credentials and identifiers redacted.
 */
export function redactLog(text: string): string {
	return text
		.replaceAll(SECRET_ASSIGNMENT, '$1"<removed>"')
		.replaceAll(/-epicuserid=("[^"]*"|\S+)/gi, "-epicuserid=<removed>")
		.replaceAll(/\b(token|bearer)([ :=]+)[^\s,;"'<]+/gi, "$1$2<removed>")
		.replaceAll(/\beyJ[\w-]{8,}\.[\w-]+\.[\w-]*/g, "<removed>")
		.replaceAll(/\b7656119\d{10}\b/g, "<removed>")
		// Avoid retrying every suffix of a long source line that contains no email address.
		.replaceAll(/(?<![\w.+-])[\w.+-]+@[\w-]+\.[\w.-]+/g, "<removed>")
		.replaceAll(
			/\b(?:(?:25[0-5]|2[0-4]\d|1?\d?\d)\.){3}(?:25[0-5]|2[0-4]\d|1?\d?\d)\b/g,
			(ip) => (ip === "127.0.0.1" || ip === "0.0.0.0" ? ip : "<ip>"),
		);
}
