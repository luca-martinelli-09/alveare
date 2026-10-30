<script lang="ts">
	// Pagina o oggetto che non c'è (eliminato, mai creato, link sbagliato) o non è per te.
	import * as Empty from '$lib/components/ui/empty';
	import { Button } from '$lib/components/ui/button';
	import SearchXIcon from '@lucide/svelte/icons/search-x';
	import LockIcon from '@lucide/svelte/icons/lock';

	let {
		title,
		description,
		href = '/',
		label = 'Torna alla panoramica',
		forbidden = false
	}: { title: string; description: string; href?: string; label?: string; forbidden?: boolean } = $props();
</script>

<Empty.Root class="bg-card flex-none border">
	<Empty.Header>
		<Empty.Media variant="icon">{#if forbidden}<LockIcon />{:else}<SearchXIcon />{/if}</Empty.Media>
		<Empty.Title>{title}</Empty.Title>
		<Empty.Description>{description}</Empty.Description>
	</Empty.Header>
	<Empty.Content><Button {href}>{label}</Button></Empty.Content>
</Empty.Root>
