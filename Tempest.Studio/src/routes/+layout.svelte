<script lang="ts">
	import "../app.css";
	// @ts-ignore
	import "@fontsource-variable/ubuntu-sans-mono";
	import ToastStack from "$lib/components/ToastStack.svelte";
	import { assembly } from "$lib/studio/assembly.svelte";
	import { rpc } from "$lib/studio/rpc";
	import { studio } from "$lib/studio/state.svelte";

	const { children } = $props();

	// The MCP server of the host drives the renderer through this (electron/src/mcp.ts).
	(window as unknown as { __tempest: unknown }).__tempest = { studio, assembly, rpc };
	// Dev builds expose the store for the screenshot harness (electron/src/smoke.ts).
	if (import.meta.env.DEV) {
		(window as unknown as { studio: unknown; assembly: unknown }).studio = studio;
		(window as unknown as { studio: unknown; assembly: unknown }).assembly = assembly;
		(window as unknown as { rpc: unknown }).rpc = rpc;
		void Promise.all([
			import("$lib/studio/session.svelte"),
			import("$lib/studio/ui.svelte"),
			import("$lib/studio/tabs.svelte"),
			import("$lib/studio/commands.svelte"),
			import("$lib/studio/nav.svelte"),
			import("$lib/studio/cooker.svelte"),
		]).then(([s, u, t, c, n, k]) => {
			Object.assign(window, {
				session: s.session,
				ui: u.ui,
				tabs: t.tabs,
				commands: c.commands,
				nav: n.nav,
				cooker: k.cooker,
			});
		});
	}
</script>

<div class="bg-base-100 text-base-content h-screen w-screen overflow-hidden">
	{@render children?.()}
</div>
<ToastStack />
