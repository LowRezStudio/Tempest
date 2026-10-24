<script lang="ts">
	import { m } from "$lib/paraglide/messages";
	import type { SectionTracker } from "./sections.svelte";

	interface Props {
		tracker: SectionTracker;
	}

	let { tracker }: Props = $props();
</script>

<!-- The side column's list of a page's panels, after the mod website's editor pages -->
<nav aria-label={m.settings_sections()}>
	<ol class="flex flex-col gap-0.5 text-sm">
		{#each tracker.sections as s (s.id)}
			{@const Icon = s.icon}
			<li>
				<button
					type="button"
					class={[
						"rounded-field flex w-full cursor-pointer items-center gap-2 px-2 py-1 text-left",
						tracker.current === s.id
							? "bg-primary/10 font-medium"
							: "hover:bg-base-300",
						s.tone === "danger" && "text-error",
					]}
					aria-current={tracker.current === s.id ? "location" : undefined}
					onclick={() => tracker.jump(s.id)}
				>
					<Icon
						size={14}
						class={s.tone === "danger" ? "shrink-0" : "text-primary shrink-0"}
					/>
					<span class="truncate">{s.title}</span>
				</button>
			</li>
		{/each}
	</ol>
</nav>
