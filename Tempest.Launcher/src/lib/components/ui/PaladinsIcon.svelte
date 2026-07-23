<script lang="ts">
	import iconSrc from "$lib/assets/paladins-icon.png";
	import type { IconProps } from "@lucide/svelte";

	let {
		size = 24,
		color = "currentColor",
		class: className = "",
	}: IconProps = $props();

	let sizePx = $derived(typeof size === "number" ? `${size}px` : size);

	// Unique mask ID so multiple instances on the same page don't clash
	let maskId = $state("");
	$effect(() => {
		maskId = `paladins-mask-${Math.random().toString(36).slice(2, 9)}`;
	});
</script>

<svg
	width={sizePx}
	height={sizePx}
	viewBox="0 0 259 264"
	class={`paladins-icon lucide-icon lucide ${className}`}
	xmlns="http://www.w3.org/2000/svg"
	aria-hidden="true"
>
	{#if maskId}
		<defs>
			<mask id={maskId} style="mask-type: alpha;">
				<image href={iconSrc} width="259" height="264" />
			</mask>
		</defs>
		<rect width="259" height="264" fill={color} mask="url(#{maskId})" />
	{/if}
</svg>
