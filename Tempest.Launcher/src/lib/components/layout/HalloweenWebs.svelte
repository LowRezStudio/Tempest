<script lang="ts">
	// A corner web: spokes out of the corner and threads sagging toward it between them.
	const spokes = 7;
	const angle = (i: number) => (Math.PI / 2) * (1 + i / (spokes - 1));
	const at = (r: number, a: number) =>
		`${(100 + r * Math.cos(a)).toFixed(1)} ${(r * Math.sin(a)).toFixed(1)}`;
	let web = "";
	for (let i = 0; i < spokes; i++) web += `M100 0L${at(100, angle(i))}`;
	for (const r of [16, 30, 45, 60, 76, 92]) {
		for (let i = 0; i < spokes - 1; i++) {
			const sag = at(r * 0.82, (angle(i) + angle(i + 1)) / 2);
			web += `M${at(r, angle(i))}Q${sag} ${at(r, angle(i + 1))}`;
		}
	}
</script>

<div class="pointer-events-none absolute inset-0 overflow-hidden" aria-hidden="true">
	<svg class="text-base-content/25 absolute top-0 right-0 size-32" viewBox="0 0 100 100">
		<path d={web} fill="none" stroke="currentColor" stroke-width="0.7" />
	</svg>
	<svg
		class="text-base-content/25 absolute right-0 bottom-0 size-40 -scale-y-100"
		viewBox="0 0 100 100"
	>
		<path d={web} fill="none" stroke="currentColor" stroke-width="0.6" />
	</svg>
</div>
