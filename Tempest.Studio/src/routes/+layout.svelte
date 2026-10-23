<script lang="ts">
	import "../app.css";
	// @ts-ignore
	import "@fontsource-variable/ubuntu-sans-mono";
	import ToastStack from "$lib/components/ToastStack.svelte";
	import { studio } from "$lib/studio/state.svelte";

	const { children } = $props();

	// Dev builds expose the store for the screenshot harness (electron/src/smoke.ts).
	if (import.meta.env.DEV) {
		(window as unknown as { studio: unknown }).studio = studio;
	}
</script>

<div class="bg-base-100 text-base-content h-screen w-screen overflow-hidden">
	{@render children?.()}
</div>
<ToastStack />
