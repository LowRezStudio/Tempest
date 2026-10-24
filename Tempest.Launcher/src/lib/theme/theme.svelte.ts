import { theme, customThemeCSS } from "$lib/stores/settings.svelte";

const lightQuery = matchMedia("(prefers-color-scheme: light)");
let systemLight = $state(lightQuery.matches);
lightQuery.addEventListener("change", (e) => (systemLight = e.matches));

// daisyUI's own fallback for no data-theme only follows a dark device, so "system" is resolved here.
export function systemTheme(): "mocha" | "latte" {
	return systemLight ? "latte" : "mocha";
}

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
		const current = theme.value;
		document.documentElement.setAttribute(
			"data-theme",
			current === "system" ? systemTheme() : current,
		);
	});

	// Scoped to [data-theme="custom"], so it is harmless until picked and lets the
	// Settings preview show the custom theme beforehand.
	$effect(() => {
		injectCustomThemeStyle(customThemeCSS.value);
	});
});
