import { theme, customThemeCSS } from "$lib/stores/settings.svelte";

function injectCustomThemeStyle(css: string) {
	const existing = document.querySelector("#custom-theme-style");
	if (existing) existing.remove();

	if (!css.trim()) return;

	const vars: string[] = [];
	for (const line of css.split("\n")) {
		const t = line.trim();
		if (t.startsWith("--") && t.includes(":")) {
			vars.push(t);
		}
	}

	if (vars.length === 0) return;

	const style = document.createElement("style");
	style.id = "custom-theme-style";
	style.textContent = `[data-theme="custom"] {\n${vars.join("\n")}\n}`;
	document.head.append(style);
}

$effect.root(() => {
	$effect(() => {
		const currentTheme = theme.value;
		const customCSS = customThemeCSS.value;

		if (currentTheme === "system") {
			document.documentElement.removeAttribute("data-theme");
		} else if (currentTheme === "custom") {
			document.documentElement.setAttribute("data-theme", "custom");
		} else {
			document.documentElement.setAttribute("data-theme", currentTheme);
		}

		injectCustomThemeStyle(currentTheme === "custom" ? customCSS : "");
	});
});
