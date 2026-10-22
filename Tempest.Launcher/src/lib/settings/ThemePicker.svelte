<script lang="ts">
	import { Check } from "@lucide/svelte";
	import { m } from "$lib/paraglide/messages";
	import { theme, type Theme } from "$lib/stores/settings.svelte";
	import { systemTheme } from "$lib/theme/theme.svelte";

	const choices: { value: Theme; label: () => string; hint: () => string }[] = [
		{ value: "system", label: m.settings_theme_system, hint: m.settings_theme_system_hint },
		{ value: "mocha", label: m.settings_theme_mocha, hint: m.settings_theme_mocha_hint },
		{ value: "latte", label: m.settings_theme_latte, hint: m.settings_theme_latte_hint },
		{ value: "legacy", label: m.settings_theme_legacy, hint: m.settings_theme_legacy_hint },
		{ value: "custom", label: m.settings_theme_custom, hint: m.settings_theme_custom_hint },
	];
</script>

<div
	class="grid grid-cols-2 gap-2 @md:grid-cols-3 @2xl:grid-cols-5"
	role="radiogroup"
	aria-label={m.settings_theme()}
>
	{#each choices as c (c.value)}
		{@const active = theme.value === c.value}
		<button
			type="button"
			role="radio"
			aria-checked={active}
			class={[
				"rounded-field border-base-300 hover:border-primary/60 flex cursor-pointer flex-col overflow-hidden border text-left transition-colors",
				active && "border-primary! ring-primary/30 ring-2",
			]}
			onclick={() => (theme.value = c.value)}
		>
			<!-- A tiny page in that theme: its own data-theme scopes the colours to this box -->
			<span
				data-theme={c.value === "system" ? systemTheme() : c.value}
				class="bg-base-100 flex h-16 flex-col gap-1 p-2"
			>
				<span class="bg-base-300 h-2 w-full rounded-sm"></span>
				<span class="flex flex-1 gap-1">
					<span class="bg-base-200 border-base-300 flex-1 rounded-sm border"></span>
					<span class="flex w-1/3 flex-col gap-1">
						<span class="bg-primary h-2 rounded-sm"></span>
						<span class="bg-accent h-2 w-2/3 rounded-sm"></span>
					</span>
				</span>
			</span>
			<span class="bg-base-100 border-base-300 flex items-center gap-2 border-t px-2 py-1.5">
				<span class="min-w-0 flex-1">
					<span class="block truncate text-sm font-medium">{c.label()}</span>
					<span class="text-base-content/60 block truncate text-xs">{c.hint()}</span>
				</span>
				{#if active}<Check size={14} class="text-primary shrink-0" />{/if}
			</span>
		</button>
	{/each}
</div>
