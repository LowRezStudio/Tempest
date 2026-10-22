// Remove soft-wrap boundaries while preserving breaks between log entries.
export function unwrappedSelection(fragment: DocumentFragment): string {
	const rows = fragment.querySelectorAll<HTMLElement>(".terminal-row");
	if (rows.length === 0) return fragment.textContent ?? "";
	return Array.from(
		rows,
		(row, index) =>
			`${index > 0 && row.dataset.first === "true" ? "\n" : ""}${row.textContent ?? ""}`,
	).join("");
}
