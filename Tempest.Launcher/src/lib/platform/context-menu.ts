import { m } from "$lib/paraglide/messages";
import { showContextMenu, type ContextMenuAction } from "$lib/platform/api";

// Match the fields that can host a caret, including contenteditable regions.
const EDITABLE = "input, textarea, [contenteditable]:not([contenteditable='false'])";

// The app suppresses the browser context menu globally. Surfaces that show
// selectable text call this instead so the host can pop the native menu.
export function openTextContextMenu(
	event: MouseEvent,
	options: {
		/** Whether the surface can select its whole contents (e.g. a long log). */
		canSelectAll?: boolean;
	} = {},
): Promise<ContextMenuAction | null> {
	event.preventDefault();
	event.stopPropagation();

	const target = event.target;
	const field =
		target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement ? target : null;
	const editable =
		field !== null || (target instanceof Element && target.closest(EDITABLE) !== null);
	const selected = field
		? (field.selectionStart ?? 0) !== (field.selectionEnd ?? 0)
		: (window.getSelection()?.toString().length ?? 0) > 0;

	return showContextMenu({
		editable,
		canCopy: selected,
		canSelectAll: options.canSelectAll ?? false,
		labels: { copy: m.common_copy(), selectAll: m.common_select_all() },
	});
}
