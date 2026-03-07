## MCP Tools

Use the Svelte MCP server for Svelte 5 / SvelteKit documentation:

1. **list-sections** - Call FIRST to discover available docs sections.
2. **get-documentation** - Fetch ALL relevant sections for the user's task.
3. **svelte-autofixer** - MUST run on all Svelte code before presenting it. Repeat until clean.
4. **playground-link** - Only after user confirms, and NEVER if code was written to files.

---

## Svelte 5 (REQUIRED)

Legacy Svelte 3/4 syntax is NOT allowed.

| Correct (Svelte 5)                  | WRONG (Legacy)                        |
| ----------------------------------- | ------------------------------------- |
| `let count = $state(0)`             | `let count = 0` with `$:` reactivity |
| `let doubled = $derived(count * 2)` | `$: doubled = count * 2`             |
| `let { name } = $props()`           | `export let name`                    |
| `$effect(() => {...})`              | `onMount()`, `$:` for side effects   |
| `onclick={handler}`                 | `on:click={handler}`                 |
| `{@render children()}`              | `<slot />`                           |

### Props

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

### Snippets (not slots)

```svelte
<script lang="ts">
	import type { Snippet } from "svelte";
	interface Props { header: Snippet; children: Snippet }
	let { header, children }: Props = $props();
</script>

<header>{@render header()}</header>
<main>{@render children()}</main>
```

### Shared State

Global state uses **nanostores** (`@nanostores/persistent` for persistence) in `src/lib/stores/`. Use `.svelte.ts` with `$state` only for component-local shared state.

---

## Tailwind CSS v4 (REQUIRED)

Use `@import "tailwindcss"` (NOT `@tailwind base/components/utilities`). Configure theme in CSS with `@theme`, not JavaScript. Use `@utility` for custom utilities (NOT `@layer components`).

### Renamed Utilities

| v4 (CORRECT)     | v3 (WRONG)     |
| ---------------- | -------------- |
| `shadow-xs`      | `shadow-sm`    |
| `shadow-sm`      | `shadow`       |
| `rounded-xs`     | `rounded-sm`   |
| `rounded-sm`     | `rounded`      |
| `outline-hidden` | `outline-none` |
| `ring-3`         | `ring`         |

### Other v4 Changes

- CSS vars: `bg-(--color)` not `bg-[--color]`
- Important: `flex!` not `!flex`
- Borders/rings default to `currentColor` - always specify colors explicitly
- Variants apply left-to-right (opposite of v3)

---

## daisyUI 5

See `Tempest.Launcher/docs/daisyui.md` for full component reference.

1. **NEVER make up components.** Check docs for available class names.
2. **NEVER use `bg-*`/`text-*` on buttons.** Use `btn-accent`, `btn-neutral`, `btn-ghost`, etc.
3. **NEVER add spacing utilities to buttons.** They have built-in spacing.
4. **Use `*-accent` instead of `*-primary`/`*-secondary`** for highlighted elements.
5. **Use `bg-base-100`/`200`/`300`** for backgrounds.

---

## Component Policy

Use components from `Tempest.Launcher/src/lib/components/`. Check for existing components before creating new ones. New components go in `src/lib/components/` with the appropriate subdirectory (`ui/`, `sidebar/`, `library/`, etc.). Components must follow `$lib/styles/global.css`. Do NOT create one-off components inside route folders.

---

## Documentation Policy

NEVER create `.md` files in the project root or source directories. If explicitly requested, place them in `Tempest.Launcher/docs/`.
