<script lang="ts">
	import { Flag, RotateCcw, Terminal, Trash2 } from "@lucide/svelte";
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

<div class="flex flex-col gap-6">
	<div class="alert alert-warning">
		<Flag size={20} />
		<div>
			<h3 class="font-semibold">{m.settings_feature_flags()}</h3>
			<p class="text-sm opacity-90">{m.settings_feature_flags_description()}</p>
		</div>
	</div>

	<section class="flex flex-col gap-2">
		<h4 class="text-xs font-semibold uppercase opacity-60">
			{m.settings_feature_flags_known()}
		</h4>
		{#each knownFlags as flag (flag.name)}
			<label
				class="rounded-box bg-base-200 flex cursor-pointer items-center justify-between gap-3 p-3"
			>
				<div class="flex min-w-0 flex-col gap-0.5">
					<span class="label-text font-mono text-sm">{flag.name}</span>
					<span class="text-xs opacity-60">{flag.description}</span>
				</div>
				<input
					type="checkbox"
					class="toggle toggle-accent"
					checked={flag.enabled}
					onchange={() => toggleFlag(flag.name)}
				/>
			</label>
		{/each}
	</section>

	{#if unknownFlags.length > 0}
		<section class="flex flex-col gap-2">
			<h4 class="text-xs font-semibold uppercase opacity-60">
				{m.settings_feature_flags_unknown()}
			</h4>
			{#each unknownFlags as flag (flag.name)}
				<div class="rounded-box bg-base-200 flex items-center justify-between gap-3 p-3">
					<div class="flex min-w-0 flex-col gap-0.5">
						<span class="label-text font-mono text-sm">{flag.name}</span>
						<span class="font-mono text-xs opacity-60">
							{m.settings_feature_flags_value()}: {String(flag.value)}
						</span>
					</div>
					<div class="flex shrink-0 items-center gap-2">
						<input
							type="checkbox"
							class="toggle toggle-sm toggle-accent"
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
					</div>
				</div>
			{/each}
		</section>
	{/if}

	<div class="flex flex-wrap items-center gap-2">
		<button
			type="button"
			class="btn btn-error btn-sm"
			onclick={resetAll}
			disabled={!hasAnyFlag}
		>
			<RotateCcw size={14} />
			{m.settings_feature_flags_reset()}
		</button>
	</div>

	<div class="bg-base-200 rounded-box flex items-start gap-2 px-3 py-2">
		<Terminal size={16} class="mt-0.5 shrink-0 opacity-60" />
		<p class="text-xs opacity-60">{m.settings_feature_flags_console_hint()}</p>
	</div>
</div>
