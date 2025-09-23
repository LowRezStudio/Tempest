<script lang="ts">
	import type { Icon } from "@lucide/svelte";
	import { open } from "@tauri-apps/plugin-dialog";
	import type { HTMLInputAttributes } from "svelte/elements";
	import Button from "./Button.svelte";
	import Input from "./Input.svelte";

	interface Props extends HTMLInputAttributes {
		placeholder?: string;
		icon?: typeof Icon;
		type: "file" | "folder";
		multiple?: boolean;
		value?: string | string[] | null;
	}

	let { placeholder, icon, type = "file", multiple, value = $bindable() }: Props = $props();

	function formatDisplay(v: string | string[] | null | undefined) {
		if (Array.isArray(v)) return v.join(", ");
		return v ?? "";
	}

	function parseInput(text: string) {
		if (multiple) {
			// split by commas or newlines, trim, and remove empties
			return text
				.split(/[,\n]/)
				.map((s) => s.trim())
				.filter(Boolean);
		}
		return text;
	}

	const inputText = $derived(formatDisplay(value));

	const openSelector = async () => {
		const newPath = await open({
			multiple,
			directory: type != "file",
		});

		if (newPath) {
			value = newPath;
		}
	};

	const handleInput = (e: Event) => {
		const target = e.currentTarget as HTMLInputElement;
		value = parseInput(target.value) as typeof value;
	};
</script>

<div class="path-picker">
	<Input {placeholder} value={inputText} oninput={handleInput} />
	<Button {icon} onclick={openSelector}>Browse</Button>
</div>

<style>
	.path-picker {
		display: flex;
		gap: 0.5rem;
		align-items: center;
	}

	.path-picker :global(input) {
		flex: 1 1 auto;
		min-width: 0; /* allow shrinking in flex */
	}
</style>
