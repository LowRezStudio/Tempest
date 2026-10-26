// Arrow-key navigation shared by the list panes: the handler runs on the
// pane container (events bubble from the focused row), ignores keys typed
// into inputs, and moves the selection through the visible rows.

/** True when the key was pressed inside a text field, where the pane must not act. */
export function inField(event: KeyboardEvent): boolean {
	const t = event.target as HTMLElement | null;
	return !!t?.closest("input, textarea, select, [contenteditable=true], .cm-editor");
}

/** Index after a step through `count` items, clamped. */
export function step(current: number, delta: number, count: number): number {
	if (count === 0) return -1;
	if (current < 0) return delta > 0 ? 0 : count - 1;
	return Math.max(0, Math.min(count - 1, current + delta));
}

/** Focus and reveal the element that marks a row (`[data-*]` selector). */
export function reveal(selector: string, focus = true): void {
	requestAnimationFrame(() => {
		const el = document.querySelector<HTMLElement>(selector);
		if (!el) return;
		el.scrollIntoView({ block: "nearest" });
		if (focus) el.focus({ preventScroll: true });
	});
}
