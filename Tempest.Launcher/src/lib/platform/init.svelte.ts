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

	document.addEventListener("contextmenu", handleContextMenu);
	document.addEventListener("dragstart", handleDragStart);
	document.addEventListener("drop", handleDrop);

	return () => {
		document.removeEventListener("contextmenu", handleContextMenu);
		document.removeEventListener("dragstart", handleDragStart);
		document.removeEventListener("drop", handleDrop);
	};
});
