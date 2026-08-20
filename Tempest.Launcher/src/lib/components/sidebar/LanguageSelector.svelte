<script lang="ts">
	import { Globe } from "@lucide/svelte";
	import { Popover, Tooltip } from "bits-ui";
	import { m } from "$lib/paraglide/messages";
	import { locales } from "$lib/paraglide/runtime";
	import { localeLabels, localeState } from "$lib/stores/locale.svelte";
	import type { Locale } from "$lib/paraglide/runtime";

	let open = $state(false);

	function selectLocale(locale: Locale) {
		open = false;
		localeState.set(locale);
	}
</script>

<Popover.Root bind:open>
	<Tooltip.Root delayDuration={150} disableHoverableContent disabled={open}>
		<Tooltip.Trigger>
			{#snippet child({ props })}
				<Popover.Trigger
					{...props}
					class="btn btn-square btn-ghost"
					aria-label={m.common_language()}
				>
					<Globe size={20} />
				</Popover.Trigger>
			{/snippet}
		</Tooltip.Trigger>
		<Tooltip.Portal>
			<Tooltip.Content
				side="right"
				sideOffset={8}
				class="bg-neutral text-neutral-content z-50 rounded-lg px-2.5 py-1.5 text-xs font-semibold shadow-md transition-opacity duration-100 data-[state=closed]:opacity-0 data-[state=open]:opacity-100"
			>
				<Tooltip.Arrow class="fill-neutral" />
				{m.common_language()}
			</Tooltip.Content>
		</Tooltip.Portal>
	</Tooltip.Root>
	<Popover.Portal>
		<Popover.Content
			class="bg-base-300 rounded-box border-base-content/10 popover-animate z-50 min-w-[10rem] border p-1 shadow-xl"
			align="start"
			sideOffset={4}
		>
			<ul role="menu" class="menu w-40">
				{#each locales as loc}
					<li>
						<button
							class="flex items-center gap-2"
							class:active={loc === localeState.current}
							onclick={() => selectLocale(loc)}
						>
							{#if loc === localeState.current}
								<span class="text-accent">✓</span>
							{:else}
								<span class="w-4"></span>
							{/if}
							<span
								>{localeLabels[loc]?.flag ?? ""}
								{localeLabels[loc]?.label ?? loc.toUpperCase()}</span
							>
						</button>
					</li>
				{/each}
			</ul>
		</Popover.Content>
	</Popover.Portal>
</Popover.Root>
