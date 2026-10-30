<script lang="ts">
	// Il marchio di Alveare: sei celle esagonali a piramide (una "A" a nido d'ape), quella in cima in ambra. Anche favicon.
	import { cn } from '$lib/utils';
	let { class: className = '' }: { class?: string } = $props();
</script>

<svg viewBox="0 0 32 32" class={cn('size-7 shrink-0', className)} aria-hidden="true">
	<rect width="32" height="32" rx="7" class="fill-foreground" />
	<g stroke-width="1.4" stroke-linejoin="round">
		<path d="M16.00 4.90 L19.38 6.85 L19.38 10.75 L16.00 12.70 L12.62 10.75 L12.62 6.85Z" class="fill-brand stroke-brand" />
		<path d="M11.84 12.10 L15.22 14.05 L15.22 17.95 L11.84 19.90 L8.47 17.95 L8.47 14.05Z M20.16 12.10 L23.53 14.05 L23.53 17.95 L20.16 19.90 L16.78 17.95 L16.78 14.05Z M7.69 19.30 L11.06 21.25 L11.06 25.15 L7.69 27.10 L4.31 25.15 L4.31 21.25Z M16.00 19.30 L19.38 21.25 L19.38 25.15 L16.00 27.10 L12.62 25.15 L12.62 21.25Z M24.31 19.30 L27.69 21.25 L27.69 25.15 L24.31 27.10 L20.94 25.15 L20.94 21.25Z" class="fill-background stroke-background" />
	</g>
</svg>
