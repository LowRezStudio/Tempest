You are able to use the Svelte MCP server, where you have access to comprehensive Svelte 5 and SvelteKit documentation. Here's how to use the available tools effectively:

## Available MCP Tools

### 1. list-sections

Use this FIRST to discover all available documentation sections. Returns a structured list with titles, use_cases, and paths.
When asked about Svelte or SvelteKit topics, ALWAYS use this tool at the start of the chat to find relevant sections.

### 2. get-documentation

Retrieves full documentation content for specific sections. Accepts single or multiple sections.
After calling the list-sections tool, you MUST analyze the returned documentation sections (especially the use_cases field) and then use the get-documentation tool to fetch ALL documentation sections that are relevant for the user's task.

### 3. svelte-autofixer

Analyzes Svelte code and returns issues and suggestions.
You MUST use this tool whenever writing Svelte code before sending it to the user. Keep calling it until no issues or suggestions are returned.

### 4. playground-link

Generates a Svelte Playground link with the provided code.
After completing the code, ask the user if they want a playground link. Only call this tool after user confirmation and NEVER if code was written to files in their project.

---

## Svelte 5 Code Standards

This project uses **Svelte 5**. You MUST use modern Svelte 5 patterns. Legacy Svelte 3/4 syntax is NOT allowed.

### Runes (REQUIRED)

Always use runes for reactivity:

| Correct (Svelte 5)                  | WRONG (Legacy - DO NOT USE)          |
| ----------------------------------- | ------------------------------------ |
| `let count = $state(0)`             | `let count = 0` with `$:` reactivity |
| `let doubled = $derived(count * 2)` | `$: doubled = count * 2`             |
| `let { name } = $props()`           | `export let name`                    |
| `$effect(() => {...})`              | `onMount()`, `$:` for side effects   |

### Props Pattern

```svelte
<script lang="ts">
	interface Props {
		title: string;
		count?: number;
		children?: Snippet;
	}
	let { title, count = 0, children }: Props = $props();
</script>
```

### State Management

```svelte
<script lang="ts">
	// Local reactive state
	let count = $state(0);

	// Derived/computed values
	let doubled = $derived(count * 2);

	// Complex derivations
	let total = $derived.by(() => {
		let sum = 0;
		for (const item of items) sum += item.value;
		return sum;
	});

	// Side effects (use sparingly)
	$effect(() => {
		console.log("count changed:", count);
		return () => {
			// cleanup function (optional)
		};
	});
</script>
```

### Event Handlers

| Correct (Svelte 5)    | WRONG (Legacy)         |
| --------------------- | ---------------------- |
| `onclick={handler}`   | `on:click={handler}`   |
| `oninput={handler}`   | `on:input={handler}`   |
| `onsubmit={handler}`  | `on:submit={handler}`  |
| `onkeydown={handler}` | `on:keydown={handler}` |

### Content Composition with Snippets

Use snippets instead of slots:

```svelte
<!-- Correct - Svelte 5 snippets -->
<script lang="ts">
	import type { Snippet } from "svelte";

	interface Props {
		header: Snippet;
		children: Snippet;
	}
	let { header, children }: Props = $props();
</script>

<div class="card">
	<header>{@render header()}</header>
	<main>{@render children()}</main>
</div>
```

```svelte
<!-- Usage -->
<Card>
	{#snippet header()}
		<h2>Title</h2>
	{/snippet}

	<p>Card content goes here</p>
</Card>
```

**WRONG - Legacy slots (DO NOT USE):**

```svelte
<!-- DO NOT USE -->
<slot name="header" />
<slot />
```

### Conditional Classes (Svelte 5.16+)

Use the `class` attribute with arrays or objects:

```svelte
<!-- Correct - array syntax -->
<div class={["base-class", active && "active", size === "lg" && "text-lg"]}>

<!-- Correct - object syntax -->
<div class={{ "base-class": true, active, "text-lg": size === "lg" }}>

<!-- Avoid - legacy class directive -->
<div class:active class:text-lg={size === "lg"}>
```

### Shared State Across Components

For shared state, use `.svelte.js` or `.svelte.ts` files:

```ts
// stores/counter.svelte.ts
export const counter = $state({ count: 0 });

export function increment() {
	counter.count += 1;
}
```

---

## Tailwind CSS v4 Standards

This project uses **Tailwind CSS v4**. You MUST use v4 syntax. Legacy v3 patterns will cause build errors or incorrect styling.

### CSS Setup

```css
/* Correct - Tailwind v4 */
@import "tailwindcss";

/* WRONG - v3 directives (DO NOT USE) */
@tailwind base;
@tailwind components;
@tailwind utilities;
```

### Renamed Utilities (CRITICAL)

Many utilities have been renamed in v4. Using v3 names will produce incorrect results:

| v4 (CORRECT)     | v3 (WRONG)      | Notes                    |
| ---------------- | --------------- | ------------------------ |
| `shadow-xs`      | `shadow-sm`     | Smallest shadow          |
| `shadow-sm`      | `shadow`        | Small shadow             |
| `blur-xs`        | `blur-sm`       | Smallest blur            |
| `blur-sm`        | `blur`          | Small blur               |
| `rounded-xs`     | `rounded-sm`    | Smallest radius          |
| `rounded-sm`     | `rounded`       | Small radius             |
| `outline-hidden` | `outline-none`  | Hide outline (a11y safe) |
| `ring-3`         | `ring`          | 3px ring width           |
| `ring-1`         | (default in v4) | 1px ring width           |

### CSS Variables in Classes

```html
<!-- Correct - v4 uses parentheses -->
<div class="bg-(--brand-color) text-(--text-color)">
	<!-- WRONG - v3 bracket syntax -->
	<div class="bg-[--brand-color]"></div>
</div>
```

### Important Modifier

```html
<!-- Correct - v4 places ! at end -->
<div class="flex! bg-red-500! hover:bg-red-600!">
	<!-- WRONG - v3 places ! at start -->
	<div class="!flex !bg-red-500"></div>
</div>
```

### Border, Divide, and Ring Colors

v4 defaults to `currentColor`, not gray. Always specify colors:

```html
<!-- Correct - explicit color -->
<div class="border border-gray-200">
	<div class="divide-y divide-gray-100">
		<div class="ring-2 ring-blue-500">
			<!-- WRONG - no color (will use currentColor, not gray) -->
			<div class="border"></div>
		</div>
	</div>
</div>
```

### Custom Utilities

```css
/* Correct - v4 @utility directive */
@utility btn {
	border-radius: var(--radius-sm);
	padding: var(--spacing-2) var(--spacing-4);
}

/* WRONG - v3 @layer (DO NOT USE) */
@layer components {
	.btn {
		@apply rounded-sm px-4 py-2;
	}
}
```

### Variant Stacking Order

v4 applies variants left-to-right (like CSS):

```html
<!-- v4: applies * first, then first -->
<ul class="*:first:pt-0">
	<!-- v3 was right-to-left (opposite) -->
</ul>
```

### Theme Configuration

Configure theme in CSS, not JavaScript:

```css
@import "tailwindcss";

@theme {
	--color-brand: oklch(0.7 0.15 200);
	--font-display: "Inter", sans-serif;
	--breakpoint-3xl: 1920px;
}
```

---

## Code Quality Workflow

When writing Svelte components:

1. Write the component using Svelte 5 runes and patterns
2. Use Tailwind v4 utility classes for styling
3. **ALWAYS run `svelte-autofixer`** before presenting code to the user
4. If issues are found, fix them and re-run autofixer
5. Repeat until no issues remain
6. Only then present the final code

The autofixer catches:

- Legacy Svelte 3/4 syntax
- Incorrect rune usage
- Deprecated patterns
- Common mistakes

---

## Quick Reference: Common Mistakes to Avoid

| Category        | WRONG             | CORRECT                   |
| --------------- | ----------------- | ------------------------- |
| State           | `let x = 0`       | `let x = $state(0)`       |
| Derived         | `$: y = x * 2`    | `let y = $derived(x * 2)` |
| Props           | `export let name` | `let { name } = $props()` |
| Events          | `on:click`        | `onclick`                 |
| Slots           | `<slot />`        | `{@render children()}`    |
| Shadow          | `shadow-sm`       | `shadow-xs`               |
| Rounded         | `rounded`         | `rounded-sm`              |
| Ring            | `ring` (3px)      | `ring-3`                  |
| Important       | `!flex`           | `flex!`                   |
| CSS Vars        | `bg-[--color]`    | `bg-(--color)`            |
| Tailwind import | `@tailwind base`  | `@import "tailwindcss"`   |

---

## Component Policy

**ALWAYS use components from `src/lib/components/`.**

When building UI:

1. First check if a suitable component already exists in `src/lib/components/`
2. If it exists, import and use it: `import Button from '$lib/components/Button.svelte'`
3. If it doesn't exist, create it in `src/lib/components/` before using it
4. Components should be reusable and follow the project's theme (`$lib/theme.css`)

**Component file structure:**

```
src/lib/components/
  Button.svelte
  Card.svelte
  Input.svelte
  Modal.svelte
  ...
```

**DO NOT:**

- Create one-off components inside route folders
- Duplicate component logic across pages
- Use inline styles when theme variables exist

---

## Documentation Policy

**NEVER create documentation files (`.md`, `README`, etc.) in the project root or source directories.**

If documentation is explicitly requested:

1. Place all documentation files in the `docs/` folder
2. Create the `docs/` folder if it doesn't exist
3. Use descriptive filenames (e.g., `docs/api-reference.md`, `docs/getting-started.md`)

Do NOT create:

- `README.md` in project root (unless explicitly asked)
- Inline documentation files alongside source code
- Any `.md` files outside of `docs/`

---

## daisyUI 5

This project uses **daisyUI 5** for UI components. You MUST follow the daisyUI 5 guidelines.

**See [docs/daisyui.md](docs/daisyui.md) for complete daisyUI 5 documentation including:**

- Installation and configuration
- All available components and their class names
- Color system and theming
- Usage rules and best practices
