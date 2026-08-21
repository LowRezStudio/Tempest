<script lang="ts">
	import { page } from "$app/state";
	import ErrorDetails from "$lib/components/ui/ErrorDetails.svelte";
	import { m } from "$lib/paraglide/messages";

	let { error }: { error: App.Error } = $props();

	const notFound = $derived(page.status === 404);
	const title = $derived(notFound ? m.error_404_title() : m.error_page_title());
	const description = $derived(notFound ? m.error_404_description() : m.error_page_description());
</script>

<svelte:head>
	<title>{title} · Tempest Launcher</title>
</svelte:head>

<div class="bg-base-100 flex min-h-screen flex-col items-center justify-center p-6 text-center">
	<p class="text-error/25 text-9xl leading-none font-black tabular-nums select-none">
		{page.status}
	</p>
	<h1 class="mt-4 text-2xl font-bold sm:text-3xl">{title}</h1>
	<p class="text-base-content/70 mt-2 max-w-md">{description}</p>

	<ErrorDetails {error} class="mt-6 w-full max-w-lg" />

	{#if notFound}
		<a href="/" class="btn btn-outline btn-accent mt-8">{m.error_go_home()}</a>
	{:else}
		<button class="btn btn-outline btn-accent mt-8" onclick={() => location.reload()}>
			{m.error_reload()}
		</button>
	{/if}
</div>
