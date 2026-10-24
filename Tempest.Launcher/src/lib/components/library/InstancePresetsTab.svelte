<script lang="ts">
	import { Check, Plus, SlidersHorizontal, Trash2 } from "@lucide/svelte";
	import EmptyState from "$lib/components/ui/EmptyState.svelte";
	import {
		activePreset,
		applyLaunchPreset,
		launchPresets,
		saveLaunchPresets,
	} from "$lib/core/presets";
	import { m } from "$lib/paraglide/messages";
	import { parseArgs } from "$lib/utils/args";
	import type { Instance, LaunchPreset } from "$lib/types/instance";

	interface Props {
		instance: Instance;
	}

	let { instance }: Props = $props();

	let presets = $derived(launchPresets.value);
	let active = $derived(activePreset(instance));

	// Arguments are edited as raw text: re-parsing on every keystroke would eat the
	// trailing space you need to type the next argument.
	let argsText = $state<Record<string, string>>({});
	const seeded = new Set<string>();
	$effect(() => {
		for (const preset of presets) {
			if (seeded.has(preset.id)) continue;
			seeded.add(preset.id);
			argsText[preset.id] = preset.args.join(" ");
		}
	});

	const update = (id: string, patch: Partial<LaunchPreset>) =>
		saveLaunchPresets(
			presets.map((preset) => (preset.id === id ? { ...preset, ...patch } : preset)),
		);

	const add = () =>
		saveLaunchPresets([...presets, { id: crypto.randomUUID(), title: "", args: [] }]);

	const remove = (id: string) => saveLaunchPresets(presets.filter((preset) => preset.id !== id));
</script>

<div class="flex flex-col gap-3">
	{#if presets.length > 0}
		<button class="btn btn-primary btn-sm self-end" onclick={add}>
			<Plus size={14} />
			{m.instance_preset_add()}
		</button>
	{/if}

	{#each presets as preset (preset.id)}
		<div
			data-panel
			class="border-base-content/10 bg-base-200 flex flex-col gap-3 rounded-xl border p-3 lg:flex-row lg:items-center"
		>
			<div class="flex min-w-0 flex-1 flex-col gap-2">
				<input
					type="text"
					class="input input-bordered input-sm w-full font-semibold"
					placeholder={m.instance_preset_title_placeholder()}
					value={preset.title}
					oninput={(event) => update(preset.id, { title: event.currentTarget.value })}
				/>
				<input
					type="text"
					class="input input-bordered input-sm w-full font-mono text-sm"
					placeholder={m.instance_launch_arguments()}
					value={argsText[preset.id] ?? ""}
					oninput={(event) => (argsText[preset.id] = event.currentTarget.value)}
					onchange={(event) =>
						update(preset.id, { args: parseArgs(event.currentTarget.value) })}
				/>
			</div>
			<div class="flex shrink-0 items-center gap-2">
				<button
					class="btn btn-sm"
					class:btn-success={active?.id === preset.id}
					onclick={() => applyLaunchPreset(instance, preset)}
				>
					<Check size={14} />
					{active?.id === preset.id
						? m.instance_preset_active()
						: m.instance_preset_use()}
				</button>
				<button
					class="btn btn-square btn-ghost btn-sm text-error"
					title={m.common_delete()}
					onclick={() => remove(preset.id)}
				>
					<Trash2 size={14} />
				</button>
			</div>
		</div>
	{:else}
		<EmptyState title={m.instance_preset_empty()} description={m.instance_presets_hint()}>
			{#snippet icon()}
				<SlidersHorizontal size={48} />
			{/snippet}
			{#snippet actions()}
				<button class="btn btn-primary btn-sm mt-2" onclick={add}>
					<Plus size={14} />
					{m.instance_preset_add()}
				</button>
			{/snippet}
		</EmptyState>
	{/each}
</div>
