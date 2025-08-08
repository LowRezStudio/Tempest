<script>
    import { createDialog, melt } from '@melt-ui/svelte';
    import { fade, fly } from 'svelte/transition';
	import { X } from "@lucide/svelte";
  
    // Props
    let {
      open = $bindable(false),
      title = 'Dialog',
      description = '',
      children,
      ...restProps
    } = $props();
  
    // Create dialog builder
    const {
      elements: { trigger, overlay, content, title: titleEl, description: descEl, close },
      states: { open: dialogOpen }
    } = createDialog({
      forceVisible: true,
      open: open,
      onOpenChange: ({ next }) => {
        open = next;
        return next;
      }
    });
  
    // Sync internal state with prop
    $effect(() => {
      dialogOpen.set(open);
    });
  </script>
  
  <!-- Trigger slot -->
  {#if children?.trigger}
    <button use:melt={$trigger} {...restProps}>
      {@render children.trigger()}
    </button>
  {/if}
  
  <!-- Dialog overlay and content -->
  {#if $dialogOpen}
    <div use:melt={$overlay} class="fixed inset-0 z-50 bg-black/50 flex items-center justify-center p-4" transition:fade={{ duration: 150 }}>
      <div
        use:melt={$content}
        class="bg-white dark:bg-gray-800 rounded-lg shadow-2xl p-6 max-w-lg w-full max-h-[90vh] overflow-y-auto relative"
        transition:fly={{ duration: 150, y: 8, opacity: 0 }}
      >
        <!-- Close button -->
        <button use:melt={$close} class="absolute right-4 top-4 p-1 rounded-sm hover:bg-gray-100 dark:hover:bg-gray-700 text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-200 flex items-center justify-center">
          <X size={16} />
        </button>
  
        <!-- Title -->
        {#if title}
          <h2 use:melt={$titleEl} class="text-lg font-semibold text-gray-900 dark:text-gray-100 mb-2 pr-8">
            {title}
          </h2>
        {/if}
  
        <!-- Description -->
        {#if description}
          <p use:melt={$descEl} class="text-gray-600 dark:text-gray-300 mb-4 leading-relaxed">
            {description}
          </p>
        {/if}
  
        <!-- Main content slot -->
        {#if children?.default}
          <div class="my-4">
            {@render children.default()}
          </div>
        {/if}
  
        <!-- Actions slot -->
        {#if children?.actions}
          <div class="flex gap-2 justify-end mt-6 pt-4 border-t border-gray-200 dark:border-gray-600">
            {@render children.actions({ close })}
          </div>
        {/if}
      </div>
    </div>
  {/if}