<script lang="ts">
	import { RotateCcw, Terminal, Trash2 } from "@lucide/svelte";
	import { m } from "$lib/paraglide/messages";
	import { FEATURE_FLAGS, flags, isFlagEnabled, type FlagName } from "$lib/stores/flags.svelte";

	const knownFlagNames = Object.keys(FEATURE_FLAGS) as FlagName[];

	const knownFlags = $derived(
		knownFlagNames.map((name) => ({
			name,
			description: FEATURE_FLAGS[name].description,
			default: FEATURE_FLAGS[name].default,
			enabled: isFlagEnabled(name),
		})),
	);

	const unknownFlags = $derived(
		Object.entries(flags.value)
			.filter(([name]) => !(name in FEATURE_FLAGS))
			.map(([name, value]) => ({ name, value, enabled: isFlagEnabled(name as FlagName) })),
	);

	const hasAnyFlag = $derived(Object.keys(flags.value).length > 0);

	function toggleFlag(name: string) {
		flags.set({ ...flags.value, [name]: !isFlagEnabled(name as FlagName) });
	}

	function removeFlag(name: string) {
		const next = { ...flags.value };
		delete next[name];
		flags.set(next);
	}

	function resetAll() {
		flags.set({});
	}
</script>

<div class="flex flex-col gap-3">
	<p class="text-base-content/60 text-sm">{m.settings_feature_flags_description()}</p>

	<ul class="flex flex-col gap-2">
		{#each knownFlags as flag (flag.name)}
			<li>
				<label
					class="bg-base-100 border-base-300 hover:border-primary/50 rounded-field flex cursor-pointer items-center gap-3 border p-3 transition-colors"
				>
					<span class="min-w-0 flex-1">
						<span class="block font-mono text-sm">{flag.name}</span>
						<span class="text-base-content/60 block text-xs">{flag.description}</span>
					</span>
					<input
						type="checkbox"
						class="toggle toggle-primary shrink-0"
						checked={flag.enabled}
						onchange={() => toggleFlag(flag.name)}
					/>
				</label>
			</li>
		{/each}
	</ul>

	{#if unknownFlags.length > 0}
		<span class="text-base-content/60 text-xs">{m.settings_feature_flags_unknown()}</span>
		<ul class="flex flex-col gap-2">
			{#each unknownFlags as flag (flag.name)}
				<li
					class="bg-base-100 border-base-300 rounded-field flex items-center gap-3 border p-3"
				>
					<span class="min-w-0 flex-1">
						<span class="block font-mono text-sm">{flag.name}</span>
						<span class="text-base-content/60 block font-mono text-xs">
							{m.settings_feature_flags_value()}: {String(flag.value)}
						</span>
					</span>
					<input
						type="checkbox"
						class="toggle toggle-sm toggle-primary shrink-0"
						aria-label={flag.name}
						checked={flag.enabled}
						onchange={() => toggleFlag(flag.name)}
					/>
					<button
						type="button"
						class="btn btn-ghost btn-xs btn-square"
						onclick={() => removeFlag(flag.name)}
						aria-label={m.settings_feature_flags_remove()}
						title={m.settings_feature_flags_remove()}
					>
						<Trash2 size={14} />
					</button>
				</li>
			{/each}
		</ul>
	{/if}

	<div class="flex items-start gap-3">
		<p class="text-base-content/60 flex flex-1 items-start gap-2 text-xs">
			<Terminal size={14} class="mt-px shrink-0" />
			{m.settings_feature_flags_console_hint()}
		</p>
		<button
			type="button"
			class="btn btn-ghost btn-xs shrink-0"
			onclick={resetAll}
			disabled={!hasAnyFlag}
		>
			<RotateCcw size={12} />
			{m.settings_feature_flags_reset()}
		</button>
	</div>
</div>
