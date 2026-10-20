<script lang="ts">
	import { FileUp, FlaskConical } from "@lucide/svelte";
	import BatchConverter from "$lib/components/mods/BatchConverter.svelte";
	import CookerOnboarding from "$lib/components/mods/CookerOnboarding.svelte";
	import SigningKeyModal from "$lib/components/mods/SigningKeyModal.svelte";
	import V1ToV2Converter from "$lib/components/mods/V1ToV2Converter.svelte";
	import Header from "$lib/components/ui/Header.svelte";
	import { isDraggingConverterFiles } from "$lib/mods/drop.svelte";
	import { m } from "$lib/paraglide/messages";
	import { cookMode } from "$lib/stores/batch.svelte";
	import { cookerOnboarded } from "$lib/stores/settings.svelte";

	// First-run setup wizard (signing key + optional API key) on the cooker.
	let showOnboarding = $state(false);
	$effect(() => {
		if (!cookerOnboarded.value && !showOnboarding) showOnboarding = true;
	});
</script>

{#if isDraggingConverterFiles.value}
	<div
		class="bg-base-100/90 pointer-events-none absolute inset-0 z-40 flex flex-col items-center justify-center backdrop-blur-sm"
		style="animation: fade-in 0.12s ease forwards;"
	>
		<div
			class="card bg-base-200/95 border-primary/50 w-full max-w-sm border-2 border-dashed text-center shadow-2xl"
			style="animation: pop-in 0.15s var(--ease-spring) forwards;"
		>
			<div class="card-body items-center gap-3">
				<div
					class="bg-primary/10 text-primary flex h-16 w-16 items-center justify-center rounded-xl"
				>
					<FlaskConical size={32} />
				</div>
				<h2 class="card-title justify-center">{m.converter_title()}</h2>
				<p class="text-sm opacity-70">
					{m.converter_drop_hint({
						tempest: ".tempest",
						upk: ".upk",
						pck: ".pck",
						ini: ".ini",
					})}
				</p>
			</div>
		</div>
	</div>
{/if}

<div class="bg-base-100 flex h-full flex-col">
	<Header
		title={m.converter_title()}
		activeTab={cookMode.value}
		tabs={[
			{ name: m.converter_tab_single(), value: "single" },
			{ name: m.converter_tab_batch(), value: "batch" },
		]}
		onSelectTab={(tab) => (cookMode.value = tab)}
	>
		{#snippet icon()}
			<FlaskConical size={32} class="opacity-60" />
		{/snippet}
		{#snippet subtitle()}
			<span>{m.converter_subtitle()}</span>
		{/snippet}
	</Header>

	<div class="min-h-0 flex-1">
		{#if cookMode.value === "single"}
			<V1ToV2Converter />
		{:else}
			<BatchConverter />
		{/if}
	</div>
</div>

<CookerOnboarding
	bind:open={showOnboarding}
	onclose={() => {
		cookerOnboarded.value = true;
		showOnboarding = false;
	}}
/>
