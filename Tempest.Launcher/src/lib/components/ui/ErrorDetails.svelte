<script lang="ts">
	import { m } from "$lib/paraglide/messages";

	interface Props {
		error: unknown;
		class?: string;
	}

	let { error, class: className = "" }: Props = $props();

	const text = $derived.by(() => {
		if (error instanceof Error) {
			return error.stack ?? `${error.name}: ${error.message}`;
		}
		if (
			typeof error === "object" &&
			error !== null &&
			"message" in error &&
			typeof error.message === "string"
		) {
			return error.message;
		}
		return String(error);
	});
</script>

<details class="collapse-arrow bg-base-200 collapse text-left {className}">
	<summary class="collapse-title text-sm font-medium">{m.error_details()}</summary>
	<div class="collapse-content">
		<pre class="font-mono text-xs break-words whitespace-pre-wrap">{text}</pre>
	</div>
</details>
