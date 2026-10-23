<script lang="ts">
	import { toneBadge, trustOf } from "$lib/mods/listing";
	import type { RepoVersion } from "$lib/queries/repo";

	let { latest, size = "sm" }: { latest: RepoVersion | null; size?: "xs" | "sm" } = $props();

	const trust = $derived(trustOf(latest));
	const Icon = $derived(trust.icon);
</script>

<span
	class="badge gap-1 whitespace-nowrap {toneBadge[trust.tone]} {size === 'xs'
		? 'badge-xs'
		: 'badge-sm'}"
	title={trust.hint}
>
	<Icon size={size === "xs" ? 10 : 12} />
	{trust.label}
</span>
