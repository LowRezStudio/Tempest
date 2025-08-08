<script lang="ts">
	import { page } from "$app/state";
	import { getInstance } from "$lib/state/instances.svelte";

	const id = page.params.id;
	const instance = getInstance(id);
</script>

{#if instance}
	<p>{instance.label}</p>
{/if}
