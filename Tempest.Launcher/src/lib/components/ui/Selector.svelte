<script lang="ts">
	import { open } from '@tauri-apps/plugin-dialog';
	import type { Icon } from "@lucide/svelte";
	import Button from './Button.svelte';
	import type { HTMLInputAttributes } from "svelte/elements";

	interface Props extends HTMLInputAttributes {
		placeholder?: string;
		icon?: typeof Icon;
		type: "file" | "folder";
		multiple?: boolean;
		selected?: string | string[] | null;
	}

	let {
		children,
		placeholder,
		icon,
		type = "file",
		multiple,
		selected = $bindable(),
		...props
	}: Props = $props();

	const openSelector = async () => {
		selected = await open({
			multiple,
			directory: type != "file",
		});
	};
</script>

<Button onclick={openSelector}>{placeholder}</Button>
