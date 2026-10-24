<script lang="ts">
	import { untrack } from "svelte";
	import { fade } from "svelte/transition";
	import { m } from "$lib/paraglide/messages";
	import { processesList } from "$lib/stores/processes.svelte";
	import { username } from "$lib/stores/settings.svelte";
	import { today, weatherOf, weatherOverride } from "$lib/theme/weather.svelte";
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

	const weatherQuip = {
		snow: m.home_quip_weather_snow,
		leaves: m.home_quip_weather_leaves,
		petals: m.home_quip_weather_petals,
		fireflies: m.home_quip_weather_fireflies,
	};

	const running = $derived(processesList.value.length > 0);
	const ctx = $derived.by((): GreetingContext => {
		const patch = patchOf(instance?.version);
		return {
			now: today(),
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

	// The Konami code, typed anywhere in the launcher, gets its answer here.
	let lastOverride = untrack(() => weatherOverride.value);
	$effect(() => {
		const override = weatherOverride.value;
		if (override === lastOverride) return;
		lastOverride = override;
		quip = override ? weatherQuip[weatherOf(today()).kind]() : m.home_quip_weather_off();
	});

	let rerolls = 0;
	function reroll() {
		rerolls++;
		if (rerolls === 10) {
			quip = m.home_quip_rerolls();
		} else if (rerolls === 25) {
			quip = m.home_quip_prize();
			lastOverride = true;
			weatherOverride.value = true;
		} else {
			quip = pickQuip(ctx, quip);
		}
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
