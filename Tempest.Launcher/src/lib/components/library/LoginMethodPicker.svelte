<script lang="ts">
	import { Check } from "@lucide/svelte";
	import AuthLogo from "$lib/components/ui/AuthLogo.svelte";
	import { m } from "$lib/paraglide/messages";
	import type { LoginMethod } from "./folder-import.svelte";

	interface Props {
		value?: LoginMethod;
		/** Heading over the cards. */
		label?: string;
		/** Muted line beside the heading; absent leaves it out. */
		note?: string;
		/** One line per sign-in; absent cards show only their name. */
		hints?: Partial<Record<LoginMethod, () => string>>;
	}

	let {
		value = $bindable(),
		label = m.wizard_login_prompt_title(),
		note = m.wizard_login_prompt_desc(),
		hints = {},
	}: Props = $props();

	const logins = [
		{
			method: "hirez",
			title: m.wizard_login_hirez,
		},
		{
			method: "steam",
			title: m.wizard_login_steam,
		},
		{
			method: "epic",
			title: m.wizard_login_epic,
		},
	] as const satisfies readonly { method: LoginMethod; title: () => string }[];
</script>

<fieldset>
	<legend class="mb-2 flex w-full items-baseline justify-between gap-2">
		<span class="text-base-content/60 text-xs">{label}</span>
		{#if note}
			<span class="text-base-content/50 text-xs">{note}</span>
		{/if}
	</legend>
	<div role="radiogroup" class="grid grid-cols-1 gap-2 sm:grid-cols-3">
		{#each logins as login (login.method)}
			{@const picked = value === login.method}
			{@const hint = hints[login.method]?.()}
			<button
				type="button"
				role="radio"
				aria-checked={picked}
				class={[
					"bg-base-100 rounded-field flex items-center gap-3 border p-3 text-left transition-colors",
					picked ? "border-primary ring-primary/20 ring-2" : "border-base-300",
					!picked && "hover:border-primary/50 cursor-pointer",
				]}
				onclick={() => (value = login.method)}
			>
				<span
					class={[
						"flex size-8 shrink-0 items-center justify-center rounded-full transition-colors",
						picked ? "bg-primary/15 text-primary" : "bg-base-300 text-base-content/60",
					]}
				>
					<AuthLogo provider={login.method} size={16} />
				</span>
				<span class="min-w-0 flex-1">
					<span class="block text-sm font-medium">{login.title()}</span>
					{#if hint}
						<span class="text-base-content/60 block text-xs">{hint}</span>
					{/if}
				</span>
				<Check size={14} class={["shrink-0", picked ? "text-primary" : "invisible"]} />
			</button>
		{/each}
	</div>
</fieldset>
