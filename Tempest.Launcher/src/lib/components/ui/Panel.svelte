<script lang="ts">
	import type { Settings } from "@lucide/svelte";
	import type { Snippet } from "svelte";

	interface Props {
		id?: string;
		title: string;
		icon: typeof Settings;
		tone?: "default" | "danger";
		/** Briefly draws the eye, e.g. after jumping to the panel */
		highlighted?: boolean;
		/** Sits right of the title, e.g. a small action */
		aside?: Snippet;
		children: Snippet;
		class?: string;
	}

	let {
		id,
		title,
		icon: Icon,
		tone = "default",
		highlighted = false,
		aside,
		children,
		class: className = "",
	}: Props = $props();
</script>

<!-- One section of a page, after the mod website's Panel: a bordered card with an icon, a title and room on the right -->
<section
	{id}
	class={[
		"bg-base-200 rounded-box @container min-w-0 scroll-mt-6 border p-4 ring-2 ring-transparent transition-[border-color,box-shadow] duration-500",
		tone === "danger"
			? highlighted
				? "border-error/70 ring-error/15"
				: "border-error/40"
			: highlighted
				? "border-primary/50 ring-primary/15"
				: "border-base-300",
		className,
	]}
>
	<div class="mb-3 flex min-h-6 flex-wrap items-center gap-2">
		<span
			class={[
				"flex size-5 shrink-0 items-center justify-center rounded-full",
				tone === "danger" ? "bg-error/15 text-error" : "bg-base-300",
			]}><Icon size={12} /></span
		>
		<h2 class={["text-sm font-semibold", tone === "danger" && "text-error"]}>{title}</h2>
		{#if aside}<div class="ml-auto flex items-center gap-1">{@render aside()}</div>{/if}
	</div>
	{@render children()}
</section>
