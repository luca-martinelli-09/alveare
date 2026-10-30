<script lang="ts">
	// ⌘K: siti, azioni sui siti e pagine del pannello, tutto da tastiera.
	import { goto } from '$app/navigation';
	import * as Command from '$lib/components/ui/command';
	import StatusDot from './status-dot.svelte';
	import { api, errorMessage } from '$lib/api';
	import { session } from '$lib/session.svelte';
	import { sites } from '$lib/sites.svelte';
	import { palette } from '$lib/palette.svelte';
	import { health, byHealth } from '$lib/health';
	import { toast } from 'svelte-sonner';
	import ArchiveIcon from '@lucide/svelte/icons/archive';
	import ScrollTextIcon from '@lucide/svelte/icons/scroll-text';
	import FolderIcon from '@lucide/svelte/icons/folder';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import ArrowUpRightIcon from '@lucide/svelte/icons/arrow-up-right';
	import LayoutGridIcon from '@lucide/svelte/icons/layout-grid';
	import LayoutTemplateIcon from '@lucide/svelte/icons/layout-template';
	import UsersIcon from '@lucide/svelte/icons/users';
	import ServerIcon from '@lucide/svelte/icons/server';
	import UserIcon from '@lucide/svelte/icons/user';
	import PlusIcon from '@lucide/svelte/icons/plus';

	let search = $state('');
	const list = $derived((sites.list ?? []).toSorted(byHealth));

	function go(href: string) {
		palette.open = false;
		goto(href);
	}

	async function backup(id: number, domain: string) {
		palette.open = false;
		try {
			await api.post(`/api/sites/${id}/backups`);
			toast.success(`Backup di ${domain} avviato`);
		} catch (e) {
			toast.error(errorMessage(e));
		}
	}

	const pages = $derived([
		{ label: 'Panoramica', href: '/', icon: LayoutGridIcon },
		...(session.user?.is_admin
			? [
					{ label: 'Nuovo sito', href: '/?nuovo', icon: PlusIcon },
					{ label: 'Template', href: '/templates', icon: LayoutTemplateIcon },
					{ label: 'Utenti', href: '/users', icon: UsersIcon },
					{ label: 'Server', href: '/server', icon: ServerIcon }
				]
			: []),
		{ label: 'Account', href: '/account', icon: UserIcon }
	]);
</script>

<Command.Dialog bind:open={palette.open} title="Cerca siti e azioni" description="Siti, azioni sui siti e pagine del pannello">
	<Command.Input placeholder="Cerca un sito, un'azione o una pagina" bind:value={search} />
	<Command.List>
		<Command.Empty>Nessun risultato.</Command.Empty>
		<Command.Group heading="Siti">
			{#each list as s (s.id)}
				{@const h = health(s)}
				{@const name = s.domain ?? s.slug}
				<Command.Item value={`sito ${name}`} onSelect={() => go(`/sites/${s.id}`)}>
					<StatusDot level={h.level} label={h.label} class="mx-1" />
					<span class="flex-1 truncate">{name}</span>
					<span class="text-muted-foreground text-xs">{h.label}</span>
				</Command.Item>
			{/each}
		</Command.Group>
		<!-- Le azioni sui siti compaiono solo cercando: sarebbero troppe. -->
		{#if search.trim()}
			<Command.Group heading="Azioni">
				{#each list as s (s.id)}
					{@const name = s.domain ?? s.slug}
					{#if s.url}
						<Command.Item value={`apri ${name}`} onSelect={() => ((palette.open = false), window.open(s.url!, '_blank', 'noreferrer'))}>
							<ArrowUpRightIcon /> Apri {name}
						</Command.Item>
					{/if}
					{#if session.user?.is_admin && s.backups && s.status === 'active'}
						<Command.Item value={`backup ${name}`} onSelect={() => backup(s.id, name)}><ArchiveIcon /> Backup adesso di {name}</Command.Item>
					{/if}
					<Command.Item value={`log ${name}`} onSelect={() => go(`/sites/${s.id}?tab=logs`)}><ScrollTextIcon /> Log di {name}</Command.Item>
					<Command.Item value={`file ${name}`} onSelect={() => go(`/sites/${s.id}?tab=files`)}><FolderIcon /> File di {name}</Command.Item>
					{#if s.kind === 'wordpress'}
						<Command.Item value={`aggiornamenti ${name}`} onSelect={() => go(`/sites/${s.id}?tab=updates`)}><RefreshCwIcon /> Aggiornamenti di {name}</Command.Item>
					{/if}
				{/each}
			</Command.Group>
		{/if}
		<Command.Group heading="Vai a">
			{#each pages as p (p.href)}
				<Command.Item value={`pagina ${p.label}`} onSelect={() => go(p.href)}><p.icon /> {p.label}</Command.Item>
			{/each}
		</Command.Group>
	</Command.List>
</Command.Dialog>
