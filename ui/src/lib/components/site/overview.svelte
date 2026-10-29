<script lang="ts">
	import { goto } from '$app/navigation';
	import * as Card from '$lib/components/ui/card';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import * as Dialog from '$lib/components/ui/dialog';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import JobLog from '$lib/components/app/job-log.svelte';
	import { api, errorMessage } from '$lib/api';
	import { session } from '$lib/session.svelte';
	import { toast } from 'svelte-sonner';
	import type { SiteDetail } from './types';

	let { detail }: { detail: SiteDetail } = $props();
	let deleting = $state<number | null>(null);
	const kinds = { primary: 'principale', alias: 'alias', redirect: 'redirect' };

	async function remove() {
		try {
			deleting = (await api.del<{ job: number }>(`/api/sites/${detail.site.id}`)).job;
		} catch (e) {
			toast.error(errorMessage(e));
		}
	}
</script>

<div class="grid gap-4 pt-4 md:grid-cols-2">
	<Card.Root>
		<Card.Header>
			<Card.Title>Domini</Card.Title>
			<Card.Description>Ogni hostname va aggiunto al tunnel Cloudflare verso http://caddy:80.</Card.Description>
		</Card.Header>
		<Card.Content class="flex flex-col gap-2">
			{#each detail.domains as d (d.hostname)}
				<div class="flex items-center justify-between">
					<a class="underline-offset-4 hover:underline" href={`https://${d.hostname}`} target="_blank" rel="noreferrer">{d.hostname}</a>
					<Badge variant="outline">{kinds[d.kind]}</Badge>
				</div>
			{/each}
		</Card.Content>
	</Card.Root>
	<Card.Root>
		<Card.Header>
			<Card.Title>PHP</Card.Title>
			<Card.Description>Pool dedicato con l'utente Unix {detail.site.slug}.</Card.Description>
		</Card.Header>
		<Card.Content>
			<dl class="grid grid-cols-2 gap-2 text-sm">
				<dt class="text-muted-foreground">Tipo</dt>
				<dd>{detail.site.kind} ({detail.site.template})</dd>
				<dt class="text-muted-foreground">memory_limit</dt>
				<dd>{detail.site.settings.memory_limit_mb} MB</dd>
				<dt class="text-muted-foreground">Upload massimo</dt>
				<dd>{detail.site.settings.upload_max_mb} MB</dd>
				<dt class="text-muted-foreground">Tempo massimo</dt>
				<dd>{detail.site.settings.max_execution_time_s} s</dd>
				<dt class="text-muted-foreground">Processi massimi</dt>
				<dd>{detail.site.settings.max_children}</dd>
			</dl>
		</Card.Content>
	</Card.Root>
	{#if session.user?.is_admin}
		<Card.Root class="md:col-span-2">
			<Card.Header>
				<Card.Title>Elimina sito</Card.Title>
				<Card.Description>
					Prima un backup finale, poi route, pool, file (nel cestino per 7 giorni), database e utente.
				</Card.Description>
			</Card.Header>
			<Card.Footer>
				<AlertDialog.Root>
					<AlertDialog.Trigger>
						{#snippet child({ props })}
							<Button variant="destructive" {...props}>Elimina {detail.site.slug}</Button>
						{/snippet}
					</AlertDialog.Trigger>
					<AlertDialog.Content>
						<AlertDialog.Header>
							<AlertDialog.Title>Eliminare {detail.domains[0]?.hostname}?</AlertDialog.Title>
							<AlertDialog.Description>Il sito smette di rispondere subito. Il backup finale resta per 30 giorni.</AlertDialog.Description>
						</AlertDialog.Header>
						<AlertDialog.Footer>
							<AlertDialog.Cancel>Annulla</AlertDialog.Cancel>
							<AlertDialog.Action onclick={remove}>Elimina</AlertDialog.Action>
						</AlertDialog.Footer>
					</AlertDialog.Content>
				</AlertDialog.Root>
			</Card.Footer>
		</Card.Root>
	{/if}
</div>

<Dialog.Root open={deleting !== null} onOpenChange={(o) => !o && (deleting = null)}>
	<Dialog.Content>
		<Dialog.Header>
			<Dialog.Title>Eliminazione in corso</Dialog.Title>
		</Dialog.Header>
		{#if deleting}
			<JobLog id={deleting} ondone={(j) => j.status === 'done' && goto('/')} />
		{/if}
	</Dialog.Content>
</Dialog.Root>
