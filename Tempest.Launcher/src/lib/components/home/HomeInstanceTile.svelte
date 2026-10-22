<script lang="ts">
	import PaladinsIcon from "$lib/components/ui/PaladinsIcon.svelte";
	import { relativeDate } from "$lib/mods/listing";
	import { m } from "$lib/paraglide/messages";
	import { queueItems } from "$lib/rigby/stores.svelte";
	import { processesList } from "$lib/stores/processes.svelte";
	import { getContrastColor, getInstanceColor } from "$lib/utils/color";
	import type { Instance } from "$lib/types/instance";

	interface Props {
		instance: Instance;
	}

	let { instance }: Props = $props();

	const phase = $derived(instance.state.type);
	const playable = $derived(phase === "prepared" || phase === "setup");
	const color = $derived(getInstanceColor(instance));
	const contrast = $derived(getContrastColor(color));

	const running = $derived(processesList.value.some((p) => p.instance.id === instance.id));
	const download = $derived(
		queueItems.value.find(
			(item) => item.outDir === instance.path && item.status !== "complete",
		),
	);

	// The library row's badge and last-played fact, as one line.
	const status = $derived.by(() => {
		const muted = "text-base-content/60";
		if (running) return { label: m.library_badge_running(), tone: "text-success" };
		if (phase === "setup") return { label: m.library_badge_setup(), tone: muted };
		if (phase === "paused") return { label: m.common_paused(), tone: "text-warning" };
		if (phase === "downloading") {
			if (download?.status === "error") {
				return { label: m.library_badge_failed(), tone: "text-error" };
			}
			if (download?.status === "running") {
				const percent = Math.round(download.progress?.percent ?? 0);
				return { label: m.library_badge_downloading({ percent }), tone: "text-primary" };
			}
			return { label: m.library_badge_queued(), tone: muted };
		}
		if (instance.lastPlayedAt) {
			const when = relativeDate(new Date(instance.lastPlayedAt).toISOString());
			return { label: m.library_played({ when }), tone: muted };
		}
		return { label: m.home_not_played(), tone: muted };
	});
</script>

<a
	href={playable ? `/instance/${instance.id}` : "/downloads"}
	class="hover:bg-base-content/5 rounded-field flex items-center gap-3 p-1.5 transition-colors"
>
	<span
		class="rounded-field flex size-9 shrink-0 items-center justify-center text-[11px] font-bold tabular-nums"
		class:opacity-60={!playable}
		style="background-color: {color}; color: {contrast};"
	>
		{#if instance.version}
			{instance.version}
		{:else}
			<PaladinsIcon size={22} color={contrast} />
		{/if}
	</span>
	<span class="min-w-0 flex-1">
		<span class="block truncate text-sm font-semibold">{instance.label}</span>
		<span class="block truncate text-xs tabular-nums {status.tone}">{status.label}</span>
	</span>
</a>
