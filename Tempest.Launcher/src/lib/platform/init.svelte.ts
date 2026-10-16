import { openUrl } from "@tauri-apps/plugin-opener";
import { platform } from "@tauri-apps/plugin-os";
import { polyfillCountryFlagEmojis } from "country-flag-emoji-polyfill";

$effect.root(() => {
	polyfillCountryFlagEmojis();
	document.documentElement.dataset.platform = platform();

	const handleContextMenu = (event: Event) => {
		if (!import.meta.env.DEV) {
			event.preventDefault();
		}
	};
	const handleDragStart = (event: Event) => {
		event.preventDefault();
	};
	const handleDrop = (event: Event) => {
		event.preventDefault();
	};

	// External links are opened here and nowhere else. The webview's own
	// link handling is disabled (tauri-plugin-opener's injected script,
	// Electron's window-open/navigation handlers), so a modified click
	// (Ctrl/Cmd/Shift) or a middle-click can never launch a browser.
	const externalHref = (anchor: HTMLAnchorElement): string | null => {
		if (!anchor.href) return null;
		const url = new URL(anchor.href);
		const web = url.protocol === "http:" || url.protocol === "https:";
		if (!web && url.protocol !== "mailto:" && url.protocol !== "tel:") return null;
		if (web && url.origin === location.origin) return null;
		return anchor.href;
	};
	const anchorFrom = (event: Event) =>
		event
			.composedPath()
			.find((node): node is HTMLAnchorElement => node instanceof HTMLAnchorElement);

	const handleLinkClick = (event: MouseEvent) => {
		const anchor = anchorFrom(event);
		if (!anchor) return;
		const href = externalHref(anchor);
		if (!href) return;
		event.preventDefault();
		if (event.ctrlKey || event.metaKey || event.shiftKey || event.altKey) return;
		void openUrl(href);
	};
	const handleLinkAuxClick = (event: MouseEvent) => {
		const anchor = anchorFrom(event);
		if (anchor && externalHref(anchor)) event.preventDefault();
	};

	document.addEventListener("contextmenu", handleContextMenu);
	document.addEventListener("dragstart", handleDragStart);
	document.addEventListener("drop", handleDrop);
	document.addEventListener("click", handleLinkClick);
	document.addEventListener("auxclick", handleLinkAuxClick);

	if (!import.meta.env.DEV) {
		const style = document.createElement("style");
		style.textContent =
			"body { user-select: none; } input, textarea, [contenteditable] { user-select: text; }";
		document.head.append(style);
	}

	return () => {
		document.removeEventListener("contextmenu", handleContextMenu);
		document.removeEventListener("dragstart", handleDragStart);
		document.removeEventListener("drop", handleDrop);
		document.removeEventListener("click", handleLinkClick);
		document.removeEventListener("auxclick", handleLinkAuxClick);
	};
});
