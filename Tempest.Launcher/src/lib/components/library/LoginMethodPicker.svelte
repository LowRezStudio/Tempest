<script lang="ts">
	import { Gamepad2, ShoppingBag, UserRound } from "@lucide/svelte";
	import { m } from "$lib/paraglide/messages";
	import type { LoginMethod } from "./folder-import.svelte";

	interface Props {
		value?: LoginMethod;
	}

	let { value = $bindable() }: Props = $props();

	const logins = [
		{
			method: "steam",
			Icon: Gamepad2,
			title: m.wizard_login_steam,
			hint: m.wizard_login_steam_desc,
		},
		{
			method: "epic",
			Icon: ShoppingBag,
			title: m.wizard_login_epic,
			hint: m.wizard_login_epic_desc,
		},
		{
			method: "hirez",
			Icon: UserRound,
			title: m.wizard_login_hirez,
			hint: m.wizard_login_hirez_desc,
		},
	] as const;
</script>

<fieldset>
	<legend class="mb-2 flex w-full items-baseline justify-between gap-2">
		<span class="text-sm font-medium">{m.wizard_login_prompt_title()}</span>
		<span class="text-base-content/50 text-xs">{m.wizard_login_prompt_desc()}</span>
	</legend>
	<div role="radiogroup" class="grid grid-cols-3 gap-2">
		{#each logins as o (o.method)}
			{@const picked = value === o.method}
			<button
				type="button"
				role="radio"
				aria-checked={picked}
				class={[
					"rounded-field flex cursor-pointer flex-col items-start gap-0.5 border p-2.5 text-left transition-colors",
					picked
						? "border-primary bg-primary/10"
						: "border-base-300 bg-base-100 hover:border-primary/50",
				]}
				onclick={() => (value = o.method)}
			>
				<span class="flex items-center gap-1.5 text-sm font-medium">
					<o.Icon size={14} class={picked ? "text-primary" : "text-base-content/50"} />
					{o.title()}
				</span>
				<span class="text-base-content/60 text-xs leading-snug">{o.hint()}</span>
			</button>
		{/each}
	</div>
</fieldset>
