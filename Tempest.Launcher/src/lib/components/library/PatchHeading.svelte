<script lang="ts">
	import { m } from "$lib/paraglide/messages";
	import type { Patch } from "$lib/utils/versions";

	interface Props {
		patch: Patch | undefined;
	}

	let { patch }: Props = $props();
</script>

<span class="text-base-content/60 min-w-0 truncate text-xs">
	{m.wizard_game_version()}
	{#if patch}
		<span class="text-base-content ml-1">
			<span class="font-mono font-semibold">{patch.version}</span> · {patch.name}
		</span>
	{/if}
</span>
