import { theme } from "$lib/stores/settings.svelte";

$effect.root(() => {
	$effect(() => {
		const currentTheme = theme.value;
		if (currentTheme === "system") {
			document.documentElement.removeAttribute("data-theme");
		} else {
			document.documentElement.setAttribute("data-theme", currentTheme);
		}
	});
});
