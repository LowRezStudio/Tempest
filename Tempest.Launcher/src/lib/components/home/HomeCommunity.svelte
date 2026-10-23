<script lang="ts">
	import { BookOpen, LayoutGrid, MessageCircle } from "@lucide/svelte";
	import Section from "$lib/components/ui/Section.svelte";
	import { m } from "$lib/paraglide/messages";
	import { openUrl } from "$lib/platform/api";
	import { repoBase } from "$lib/queries/repo";

	const links = $derived([
		{
			Icon: MessageCircle,
			title: m.home_discord(),
			hint: m.home_discord_hint(),
			url: "https://discord.gg/yZDuxAk4d9",
		},
		{
			Icon: BookOpen,
			title: m.home_docs(),
			hint: m.home_docs_hint(),
			url: "https://docs.lowrezstudio.com/",
		},
		{
			Icon: LayoutGrid,
			title: m.home_mod_website(),
			hint: m.home_mod_website_hint(),
			url: repoBase(),
		},
	]);
</script>

<Section title={m.home_community()}>
	<div class="flex flex-col gap-2">
		{#each links as { Icon, title, hint, url } (url)}
			<button
				type="button"
				data-panel
				class="bg-base-200 border-base-300 hover:border-primary/60 rounded-field flex cursor-pointer items-start gap-3 border p-3 text-left text-sm transition-colors"
				onclick={() => openUrl(url)}
			>
				<Icon size={16} class="text-primary mt-0.5 shrink-0" />
				<span class="min-w-0">
					<span class="block font-semibold">{title}</span>
					<span class="text-base-content/60 block text-xs">{hint}</span>
				</span>
			</button>
		{/each}
	</div>
</Section>
