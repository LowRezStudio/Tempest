import { ansiRuns, normalizeLine, type AnsiRun } from "$lib/utils/ansi";
import { RingBuffer } from "$lib/utils/ring-buffer";
import { wrapRuns } from "$lib/utils/wrap";
import type { ProcessLog } from "$lib/types/process";

/** One physical display row: a log line is one or more of these. */
export interface Row {
	runs: AnsiRun[];
	error: boolean;
	source: string;
	input: boolean;
	/** First physical row of a log line; continuations have no prefix. */
	first: boolean;
}

/** How many rows to keep before the oldest output is dropped. */
const CAPACITY = 10_000;

// Lays the chunks of one log line out as rows, prefixing the first.
function rowsOf(chunks: AnsiRun[][], error: boolean, source: string, input: boolean): Row[] {
	return chunks.map((runs, index) => ({ runs, error, source, input, first: index === 0 }));
}

/** Retain wrapped rows, appending new log IDs and resetting when the stream restarts. */
export class Scrollback {
	private rows = new RingBuffer<Row>(CAPACITY);
	private lastId = -1;

	get length(): number {
		return this.rows.length;
	}

	at(index: number): Row | undefined {
		return this.rows.at(index);
	}

	// Report evictions so the viewport can keep its scroll position.
	sync(
		logs: ReadonlyArray<ProcessLog>,
		columns: number,
		prefixOf: (source: string) => string,
	): { dropped: number; restarted: boolean } {
		const restarted =
			this.lastId >= 0 &&
			(logs.length === 0 || logs.at(-1)!.id < this.lastId || logs[0].id > this.lastId + 1);
		if (restarted) this.clear();

		let index = logs.length;
		while (index > 0 && logs[index - 1].id > this.lastId) index--;

		const before = this.rows.length;
		let added = 0;
		for (; index < logs.length; index++) {
			const entry = logs[index];
			const source = entry.source ?? "";
			const prefix = prefixOf(source);
			const chunks = wrapRuns(
				ansiRuns(normalizeLine(entry.line)),
				columns,
				columns - prefix.length,
			);
			for (const row of rowsOf(chunks, entry.error, source, false)) {
				this.rows.push(row);
				added++;
			}
			this.lastId = entry.id;
		}

		return { dropped: Math.max(0, before + added - CAPACITY), restarted };
	}

	// Appends a command the user typed, so it shows up in the console.
	echo(line: string, columns: number): void {
		for (const row of rowsOf(wrapRuns(ansiRuns(line), columns), false, "", true)) {
			this.rows.push(row);
		}
	}

	// Re-wraps every retained row after the column count changed.
	rewrap(columns: number, prefixOf: (source: string) => string): void {
		const wrapped = new RingBuffer<Row>(CAPACITY);
		for (const line of this.lines()) {
			const prefix = prefixOf(line.source);
			const chunks = wrapRuns(line.runs, columns, columns - prefix.length);
			for (const row of rowsOf(chunks, line.error, line.source, line.input)) {
				wrapped.push(row);
			}
		}

		this.rows = wrapped;
	}

	// The whole scrollback as plain text, continuation rows joined back up.
	text(prefixOf: (source: string) => string): string {
		return [...this.lines()]
			.map((line) => `${prefixOf(line.source)}${line.runs.map((run) => run.text).join("")}`)
			.join("\n");
	}

	clear(): void {
		this.rows.clear();
		this.lastId = -1;
	}

	// The retained rows, grouped back into whole log lines.
	private *lines(): Generator<Row> {
		let line: Row | undefined;
		for (let index = 0; index < this.rows.length; index++) {
			const row = this.rows.at(index)!;
			if (row.first || !line) {
				if (line) yield line;
				line = { ...row, runs: [...row.runs] };
			} else {
				line.runs.push(...row.runs);
			}
		}
		if (line) yield line;
	}
}
