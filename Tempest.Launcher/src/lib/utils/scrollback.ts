import { ansiRuns, normalizeLine, type AnsiRun } from "$lib/utils/ansi";
import { RingBuffer } from "$lib/utils/ring-buffer";
import type { ProcessLog } from "$lib/types/process";

/** One log line; the browser wraps it to the viewport width. */
export interface Line {
	id: number;
	runs: AnsiRun[];
	error: boolean;
	source: string;
	input: boolean;
}

/** How many lines to keep before the oldest output is dropped. */
const CAPACITY = 10_000;

/** Retain log lines, appending new log IDs and resetting when the stream restarts. */
export class Scrollback {
	private lines = new RingBuffer<Line>(CAPACITY);
	private lastId = -1;
	private echoId = -1;

	// A fresh array each call, so assigning it invalidates the rendered view.
	toArray(): Line[] {
		return this.lines.toArray();
	}

	// Returns true when the incoming logs are not a continuation of the retained ones.
	sync(logs: ReadonlyArray<ProcessLog>): boolean {
		const restarted =
			this.lastId >= 0 &&
			(logs.length === 0 || logs.at(-1)!.id < this.lastId || logs[0].id > this.lastId + 1);
		if (restarted) this.clear();

		let index = logs.length;
		while (index > 0 && logs[index - 1].id > this.lastId) index--;

		for (; index < logs.length; index++) {
			const entry = logs[index];
			this.lines.push({
				id: entry.id,
				runs: ansiRuns(normalizeLine(entry.line)),
				error: entry.error,
				source: entry.source ?? "",
				input: false,
			});
			this.lastId = entry.id;
		}

		return restarted;
	}

	// Appends a command the user typed, so it shows up in the console.
	echo(line: string): void {
		this.lines.push({
			id: this.echoId--,
			runs: ansiRuns(line),
			error: false,
			source: "",
			input: true,
		});
	}

	clear(): void {
		this.lines.clear();
		this.lastId = -1;
	}
}
