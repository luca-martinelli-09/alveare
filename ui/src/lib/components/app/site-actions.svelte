<script lang="ts">
	// Azioni frequenti sulla riga di un sito, senza entrare nella sua pagina.
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import { Button } from '$lib/components/ui/button';
	import { api, errorMessage, type SiteRow } from '$lib/api';
	import { session } from '$lib/session.svelte';
	import { toast } from 'svelte-sonner';
	import EllipsisIcon from '@lucide/svelte/icons/ellipsis';
	import ArrowUpRightIcon from '@lucide/svelte/icons/arrow-up-right';
	import LayoutDashboardIcon from '@lucide/svelte/icons/layout-dashboard';
	import ArchiveIcon from '@lucide/svelte/icons/archive';

	let { site }: { site: SiteRow } = $props();

	async function backup() {
		try {
			await api.post(`/api/sites/${site.id}/backups`);
			toast.success(`Backup di ${site.domain ?? site.slug} avviato`);
		} catch (e) {
			toast.error(errorMessage(e));
		}
	}
</script>

<DropdownMenu.Root>
	<DropdownMenu.Trigger>
		{#snippet child({ props })}
			<Button variant="ghost" size="icon-sm" {...props} aria-label={`Azioni per ${site.domain ?? site.slug}`}>
				<EllipsisIcon />
			</Button>
		{/snippet}
	</DropdownMenu.Trigger>
	<DropdownMenu.Content align="end">
		<DropdownMenu.Group>
			{#if site.url}
				<DropdownMenu.Item>
					{#snippet child({ props })}
						<a href={site.url} target="_blank" rel="noreferrer" {...props}><ArrowUpRightIcon /> Apri il sito</a>
					{/snippet}
				</DropdownMenu.Item>
				{#if site.kind === 'wordpress'}
					<DropdownMenu.Item>
						{#snippet child({ props })}
							<a href={`${site.url}/wp-admin/`} target="_blank" rel="noreferrer" {...props}><LayoutDashboardIcon /> Bacheca WordPress</a>
						{/snippet}
					</DropdownMenu.Item>
				{/if}
			{/if}
			{#if session.user?.is_admin && site.backups && site.status === 'active'}
				<DropdownMenu.Item onclick={backup}><ArchiveIcon /> Backup adesso</DropdownMenu.Item>
			{/if}
		</DropdownMenu.Group>
	</DropdownMenu.Content>
</DropdownMenu.Root>
