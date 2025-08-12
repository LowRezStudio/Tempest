<script lang="ts">
	import { open } from '@tauri-apps/plugin-dialog';
	import type { Icon } from "@lucide/svelte";
	import Button from './Button.svelte';
	import type { HTMLInputAttributes } from "svelte/elements";

	interface Props extends HTMLInputAttributes {
		placeholder: string;
		icon?: typeof Icon;
		type: "file" | "folder";
		multiple?: boolean;
		value?: string | string[] | null;
	}

	let {
		placeholder,
        icon,
		type = "file",
		multiple,
		value = $bindable(),
	}: Props = $props();

	const openSelector = async () => {
		value = await open({
			multiple,
			directory: type != "file",
		});
	};
</script>
<!-- dynamically adapt the length would be better -->
<Button class="text-text-color font-semibold" icon={icon} onclick={openSelector}>{placeholder.length > 51 ? placeholder.substring(0, 51) + "..." : placeholder}</Button>