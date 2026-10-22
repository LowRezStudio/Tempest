<script lang="ts">
	import {
		Activity,
		Box,
		Boxes,
		Code2,
		FileQuestion,
		Film,
		Folder,
		Image,
		Map as MapIcon,
		Music,
		Palette,
		Type,
		Zap,
	} from "@lucide/svelte";
	import { categoryFor } from "$lib/studio/classes";

	interface Props {
		className: string;
		size?: number;
	}
	const { className, size = 13 }: Props = $props();
	const category = $derived(categoryFor(className));
</script>

{#if category === "package"}
	<Folder {size} class="text-warning" />
{:else if category === "texture"}
	<Image {size} class="text-info" />
{:else if category === "material"}
	<Palette {size} class="text-success" />
{:else if category === "mesh"}
	<Box {size} class="text-primary" />
{:else if category === "sound"}
	<Music {size} class="text-accent" />
{:else if category === "swf"}
	<Film {size} class="text-accent" />
{:else if category === "anim"}
	<Activity {size} class="text-secondary-content" />
{:else if category === "level"}
	<MapIcon {size} class="text-warning" />
{:else if category === "script"}
	<Code2 {size} class="opacity-60" />
{:else if category === "physics"}
	<Zap {size} class="text-error" />
{:else if category === "font"}
	<Type {size} class="opacity-70" />
{:else if category === "actor"}
	<Boxes {size} class="opacity-70" />
{:else}
	<FileQuestion {size} class="opacity-40" />
{/if}
