<script lang="ts">
	import "../app.css";
	// @ts-ignore
	import "@fontsource-variable/ubuntu-sans-mono";
	import ToastStack from "$lib/components/ToastStack.svelte";
	import { assembly } from "$lib/studio/assembly.svelte";
	import { commands } from "$lib/studio/commands.svelte";
	import { cooker } from "$lib/studio/cooker.svelte";
	import { nav } from "$lib/studio/nav.svelte";
	import { rpc } from "$lib/studio/rpc";
	import { session } from "$lib/studio/session.svelte";
	import { studio } from "$lib/studio/state.svelte";
	import { swfTool } from "$lib/studio/swf-tool";
	import { swfDoc } from "$lib/studio/swf.svelte";
	import { tabs } from "$lib/studio/tabs.svelte";
	import { ui } from "$lib/studio/ui.svelte";

	const { children } = $props();

	// The MCP server of the host drives the renderer through this (electron/src/mcp.ts);
	// dev builds also put the stores on `window` for the screenshot harness
	// (electron/src/smoke.ts).
	const stores = {
		studio,
		assembly,
		rpc,
		session,
		ui,
		tabs,
		commands,
		nav,
		cooker,
		swfDoc,
		swfTool,
	};
	(window as unknown as { __tempest: unknown }).__tempest = stores;
	if (import.meta.env.DEV) Object.assign(window, stores);
</script>

<div class="bg-base-100 text-base-content h-screen w-screen overflow-hidden">
	{@render children?.()}
</div>
<ToastStack />
