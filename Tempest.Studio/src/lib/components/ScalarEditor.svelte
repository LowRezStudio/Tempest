<script lang="ts">
	import { studio } from "$lib/studio/state.svelte";
	import type { PropValue } from "../../../electron/src/ipc/contract";

	interface Props {
		value: PropValue;
		type: string;
		path: string;
		editable: boolean;
		onset: (path: string, value: string) => Promise<void>;
	}
	const { value, type, path, editable, onset }: Props = $props();

	let editing = $state(false);
	let draft = $state("");
	let saving = $state(false);

	/** The text form `object.set` accepts for the current value. */
	const text = $derived.by(() => {
		if (value.t === "bool") return value.v ? "True" : "False";
		if (value.t === "byte" || value.t === "int" || value.t === "float") return String(value.v);
		if (value.t === "enum" || value.t === "name" || value.t === "str") return value.v;
		if (value.t === "object") {
			return value.v.ref === 0 ? "None" : `${value.v.class}'${value.v.path}'`;
		}
		if (value.t === "delegate") return value.v.function;
		if (value.t === "raw") {
			return `<raw ${value.v.len} bytes: ${value.v.hex}${value.v.len > 64 ? " …" : ""}>`;
		}
		return "";
	});
	const canEdit = $derived(editable && value.t !== "raw" && value.t !== "delegate");

	function start() {
		if (!canEdit) return;
		draft = value.t === "object" && value.v.ref !== 0 ? value.v.path : text;
		editing = true;
	}

	async function commit() {
		if (!editing) return;
		editing = false;
		if (draft === text) return;
		saving = true;
		try {
			await onset(path, draft);
		} finally {
			saving = false;
		}
	}

	async function toggleBool() {
		if (value.t !== "bool" || !editable) return;
		saving = true;
		try {
			await onset(path, value.v ? "False" : "True");
		} finally {
			saving = false;
		}
	}

	function onKey(event: KeyboardEvent) {
		if (event.key === "Enter") {
			event.preventDefault();
			void commit();
		} else if (event.key === "Escape") {
			editing = false;
		}
	}

	function follow() {
		if (value.t === "object" && value.v.ref > 0) void studio.select(value.v.ref - 1);
	}
</script>

{#if editing}
	<!-- svelte-ignore a11y_autofocus -->
	<input
		class="input input-xs w-full font-mono"
		bind:value={draft}
		onblur={commit}
		onkeydown={onKey}
		autofocus
		spellcheck="false"
	/>
{:else if value.t === "bool"}
	<label class="inline-flex cursor-pointer items-center gap-2">
		<input
			type="checkbox"
			class="toggle toggle-xs"
			checked={value.v}
			onchange={toggleBool}
			disabled={!editable || saving}
		/>
		<span class="opacity-70">{text}</span>
	</label>
{:else if value.t === "object"}
	<span class="inline-flex items-center gap-1">
		{#if value.v.ref > 0}
			<button
				class="link link-primary text-left hover:underline"
				onclick={follow}
				title="Go to export">{value.v.path}</button
			>
		{:else if value.v.ref < 0}
			<span class="italic opacity-80" title="import">{value.v.path}</span>
		{:else}
			<span class="opacity-50">None</span>
		{/if}
		{#if value.v.ref !== 0}
			<span class="text-[10px] opacity-40"
				>{value.v.class}{value.v.ref < 0 ? " · import" : ""}</span
			>
		{/if}
		{#if canEdit}
			<button
				class="ml-1 text-[10px] opacity-0 group-hover:opacity-60 hover:opacity-100"
				onclick={start}>edit</button
			>
		{/if}
	</span>
{:else}
	<button
		class="selectable w-full cursor-text text-left break-all {canEdit
			? 'hover:bg-base-200'
			: 'cursor-default'} {value.t === 'str' ? 'text-success' : ''} {value.t === 'name' ||
		value.t === 'enum'
			? 'text-accent'
			: ''} {value.t === 'raw' ? 'opacity-60' : ''}"
		ondblclick={start}
		onclick={(e) => {
			if (e.detail === 1 && canEdit && window.getSelection()?.toString() === "") start();
		}}
		disabled={saving}
		title={canEdit ? "Click to edit" : undefined}
	>
		{#if value.t === "str"}"{text}"{:else if value.t === "name"}'{text}'{:else}{text}{/if}
	</button>
{/if}
