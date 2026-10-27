<!-- The left strip: the editor views, then tools and help. Clicking the
	active view again hides or shows its side bar. -->
<script lang="ts">
	import {
		Database,
		Hammer,
		Keyboard,
		Music,
		Package,
		Plug,
		Search,
		Wrench,
	} from "@lucide/svelte";
	import Menu from "$lib/components/shell/Menu.svelte";
	import { commands, keyLabel } from "$lib/studio/commands.svelte";
	import { studio } from "$lib/studio/state.svelte";
	import { ui } from "$lib/studio/ui.svelte";

	const views = [
		{ id: "packages" as const, icon: Package, command: "view.packages", label: "Packages" },
		{ id: "assembly" as const, icon: Database, command: "view.assembly", label: "Assembly" },
		{ id: "mod" as const, icon: Hammer, command: "view.mod", label: "Mod cooker" },
	];

	function show(id: "packages" | "assembly" | "mod") {
		if (studio.mode === id) ui.toggle("side");
		else {
			studio.mode = id;
			ui.toggle("side", false);
		}
	}

	function tip(label: string, command: string): string {
		const c = commands.get(command);
		return c?.keys ? `${label} (${keyLabel(c.keys)})` : label;
	}
</script>

<nav
	class="border-base-300 bg-base-200 flex w-11 shrink-0 flex-col items-center gap-1 border-r py-2"
	aria-label="Views"
>
	{#each views as v (v.id)}
		<button
			class="btn btn-square btn-ghost btn-sm relative {studio.mode === v.id
				? 'text-primary'
				: 'opacity-70'}"
			onclick={() => show(v.id)}
			title={tip(v.label, v.command)}
			aria-label={v.label}
			aria-pressed={studio.mode === v.id}
		>
			{#if studio.mode === v.id}
				<span class="bg-primary absolute inset-y-1 -left-1.5 w-0.5 rounded"></span>
			{/if}
			<v.icon size={18} />
		</button>
	{/each}
	<div class="border-base-300 my-1 w-6 border-t"></div>
	<button
		class="btn btn-square btn-ghost btn-sm opacity-70"
		onclick={() => commands.run("tools.search")}
		title={tip("Search every package", "tools.search")}
		aria-label="Search"
	>
		<Search size={18} />
	</button>
	<button
		class="btn btn-square btn-ghost btn-sm opacity-70"
		onclick={() => commands.run("tools.wwise")}
		title="Wwise audio"
		aria-label="Wwise audio"
	>
		<Music size={18} />
	</button>
	<Menu
		items={[
			{ heading: "Packages" },
			"tools.replace",
			"tools.batch",
			"tools.verify",
			"tools.rewrite",
			"tools.exportSources",
			"tools.modFolder",
			"-",
			{ heading: "Assembly" },
			"asm.newContent",
			"asm.vendor",
			"asm.exportMod",
			"asm.reload",
		]}
		buttonClass="btn btn-square btn-ghost btn-sm opacity-70"
		title="Tools"
		width="w-80"
	>
		<Wrench size={18} />
	</Menu>
	<div class="flex-1"></div>
	<button
		class="btn btn-square btn-ghost btn-sm opacity-70"
		onclick={() => commands.run("tools.mcp")}
		title="MCP server: let agents drive the Studio"
		aria-label="MCP server"
	>
		<Plug size={16} />
	</button>
	<button
		class="btn btn-square btn-ghost btn-sm opacity-70"
		onclick={() => commands.run("help.shortcuts")}
		title={tip("Keyboard shortcuts", "help.shortcuts")}
		aria-label="Keyboard shortcuts"
	>
		<Keyboard size={16} />
	</button>
</nav>
