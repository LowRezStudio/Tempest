<script lang="ts">
	import { untrack } from "svelte";
	import { fade } from "svelte/transition";
	import { m } from "$lib/paraglide/messages";
	import { processesList } from "$lib/stores/processes.svelte";
	import { username } from "$lib/stores/settings.svelte";
	import { patchOf } from "$lib/utils/versions";
	import { greetingTitle, pickQuip } from "./greetings";
	import type { Instance } from "$lib/types/instance";
	import type { GreetingContext } from "./greetings";

	interface Props {
		/** The hero's instance */
		instance?: Instance;
		instances: Instance[];
	}

	let { instance, instances }: Props = $props();

	const running = $derived(processesList.value.length > 0);
	const ctx = $derived.by((): GreetingContext => {
		const patch = patchOf(instance?.version);
		return {
			now: new Date(),
			name: username.value,
			instance: instance && {
				label: instance.label,
				lastPlayedAt: instance.lastPlayedAt,
				patchName: patch?.name,
				patchDate: patch?.date,
			},
			running,
			instanceCount: instances.length,
			downloading: instances.some((i) => i.state.type === "downloading"),
		};
	});
	const title = $derived(greetingTitle(ctx));

	let quip = $state(untrack(() => pickQuip(ctx, undefined)));

	// A game starting or stopping is worth a new quip.
	let wasRunning = untrack(() => running);
	$effect(() => {
		if (running === wasRunning) return;
		wasRunning = running;
		quip = pickQuip(
			untrack(() => ctx),
			untrack(() => quip),
		);
	});

	let rerolls = 0;
	function reroll() {
		rerolls++;
		quip = rerolls === 10 ? m.home_quip_rerolls() : pickQuip(ctx, quip);
	}
</script>

<div class="min-w-0">
	<h1 class="truncate text-2xl font-bold tracking-tight">{title}</h1>
	<button
		type="button"
		class="text-base-content/60 mt-0.5 block cursor-pointer text-left text-sm"
		onclick={reroll}
	>
		{#key quip}
			<span in:fade={{ duration: 250 }}>{quip}</span>
		{/key}
	</button>
</div>
