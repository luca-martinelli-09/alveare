<script lang="ts">
	// Il marchio di Alveare: sette celle esagonali a nido d'ape, quella al centro in ambra; senza sfondo. Anche favicon.
	import { cn } from '$lib/utils';
	let { class: className = '' }: { class?: string } = $props();
</script>

<svg viewBox="0 0 32 32" class={cn('size-7 shrink-0', className)} aria-hidden="true">
	<g stroke-width="1.3" stroke-linejoin="round">
		<path d="M25.09 11.65 L28.86 13.83 L28.86 18.18 L25.09 20.35 L21.33 18.18 L21.33 13.82Z M20.55 19.52 L24.31 21.70 L24.31 26.05 L20.55 28.23 L16.78 26.05 L16.78 21.70Z M11.45 19.52 L15.22 21.70 L15.22 26.05 L11.45 28.23 L7.69 26.05 L7.69 21.70Z M6.91 11.65 L10.67 13.83 L10.67 18.18 L6.91 20.35 L3.14 18.18 L3.14 13.82Z M11.45 3.78 L15.22 5.95 L15.22 10.30 L11.45 12.48 L7.69 10.30 L7.69 5.95Z M20.55 3.78 L24.31 5.95 L24.31 10.30 L20.55 12.47 L16.78 10.30 L16.78 5.95Z" class="fill-foreground stroke-foreground" />
		<path d="M16.00 11.65 L19.77 13.83 L19.77 18.18 L16.00 20.35 L12.23 18.18 L12.23 13.82Z" class="fill-brand stroke-brand" />
	</g>
</svg>
