<script lang="ts">
	// Il marchio di Alveare: sei celle a piramide, quella in cima in ambra. Anche favicon.
	import { cn } from '$lib/utils';
	let { class: className = '' }: { class?: string } = $props();
</script>

<svg viewBox="0 0 32 32" class={cn('size-7 shrink-0', className)} aria-hidden="true">
	<rect width="32" height="32" rx="7" class="fill-foreground" />
	<rect x="13" y="5" width="6" height="6" rx="1.6" class="fill-brand" />
	<g class="fill-background">
		<rect x="9" y="13" width="6" height="6" rx="1.6" />
		<rect x="17" y="13" width="6" height="6" rx="1.6" />
		<rect x="5" y="21" width="6" height="6" rx="1.6" />
		<rect x="13" y="21" width="6" height="6" rx="1.6" />
		<rect x="21" y="21" width="6" height="6" rx="1.6" />
	</g>
</svg>
