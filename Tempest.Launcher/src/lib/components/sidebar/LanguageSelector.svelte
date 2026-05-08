<script lang="ts">
	import { Globe } from "@lucide/svelte";
	import { m } from "$lib/paraglide/messages";
	import { getLocale, locales, setLocale } from "$lib/paraglide/runtime";
	import { Popover } from "bits-ui";
	import type { Locale } from "$lib/paraglide/runtime";

	const localeLabels: Record<string, { flag: string; label: string }> = {
		en: { flag: "🇬🇧", label: "English" },
		fr: { flag: "🇫🇷", label: "Français" },
		es: { flag: "🇪🇸", label: "Español" },
		pl: { flag: "🇵🇱", label: "Polski" },
	};

	let currentLocale = $state(getLocale());
	let open = $state(false);

	function selectLocale(locale: Locale) {
		if (locale === currentLocale) return;
		currentLocale = locale;
		open = false;
		setLocale(locale);
	}
</script>

<span class="wrapper" style="anchor-scope: --lang-item;">
	<Popover.Root bind:open>
		<Popover.Trigger
			class="btn btn-square btn-ghost"
			style="anchor-name: --lang-item;"
			aria-label={m.common_language()}
		>
			<Globe size={20} />
		</Popover.Trigger>
		<div
			class="tip tooltip tooltip-right tooltip-open pointer-events-none fixed z-50 h-0 w-0"
			style="position-anchor: --lang-item; top: anchor(center); left: calc(anchor(right) + 4px); transform: translateY(-50%);"
			data-tip={m.common_language()}
			aria-hidden="true"
		></div>
		<Popover.Portal>
			<Popover.Content
				class="z-50 min-w-[10rem] bg-base-300 rounded-box p-1 shadow-xl border border-base-content/10 popover-animate"
				align="start"
				sideOffset={4}
			>
				<ul role="menu" class="menu w-40">
					{#each locales as loc}
						<li>
							<button
								class="flex items-center gap-2"
								class:active={loc === currentLocale}
								onclick={() => selectLocale(loc)}
							>
								{#if loc === currentLocale}
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
</span>

<style>
	.tip {
		opacity: 0;
		transition: opacity 100ms;
	}
	.wrapper:hover .tip,
	.wrapper:focus-within .tip {
		opacity: 1;
	}
</style>
