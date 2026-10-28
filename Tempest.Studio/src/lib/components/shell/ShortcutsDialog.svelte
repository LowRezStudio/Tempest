<!-- Every command with a shortcut, by group (F1). -->
<script lang="ts">
	import Modal from "$lib/components/Modal.svelte";
	import { commands, keyLabel } from "$lib/studio/commands.svelte";

	let { open = $bindable(false) }: { open?: boolean } = $props();

	const groups = $derived.by(() => {
		const map = new Map<string, { title: string; keys: string }[]>();
		for (const c of commands.list) {
			if (!c.keys || c.hidden) continue;
			const g = map.get(c.group) ?? [];
			g.push({ title: c.title, keys: keyLabel(c.keys) });
			map.set(c.group, g);
		}
		return [...map.entries()];
	});

	const panes = [
		["↑ ↓", "Move in a list, tree or grid"],
		["→ ←", "Expand / collapse a tree node"],
		["Home End", "First / last item"],
		["PageUp PageDown", "Previous / next page of the assembly grid"],
		["Enter", "Edit the selected cell (assembly)"],
		["Ctrl+click", "Open a package, export or reference in a new tab"],
		["Mouse back / forward", "Back / Forward"],
	];
</script>

<Modal bind:open title="Keyboard shortcuts" width="max-w-3xl">
	<div class="grid gap-x-8 gap-y-4 text-sm md:grid-cols-2">
		{#each groups as [group, list] (group)}
			<section>
				<h4 class="mb-1 text-xs font-semibold tracking-wide uppercase opacity-60">
					{group}
				</h4>
				<table class="w-full">
					<tbody>
						{#each list as s, i (`${i}:${s.title}`)}
							<tr>
								<td class="py-0.5 pr-3">{s.title}</td>
								<td class="py-0.5 text-right whitespace-nowrap"
									><kbd class="kbd kbd-sm">{s.keys}</kbd></td
								>
							</tr>
						{/each}
					</tbody>
				</table>
			</section>
		{/each}
		<section>
			<h4 class="mb-1 text-xs font-semibold tracking-wide uppercase opacity-60">
				In lists and grids
			</h4>
			<table class="w-full">
				<tbody>
					{#each panes as [k, what] (k)}
						<tr>
							<td class="py-0.5 pr-3">{what}</td>
							<td class="py-0.5 text-right whitespace-nowrap"
								><kbd class="kbd kbd-sm">{k}</kbd></td
							>
						</tr>
					{/each}
				</tbody>
			</table>
		</section>
	</div>
</Modal>
