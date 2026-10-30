<script lang="ts">
	// Esporta (gestore), clona e salva come template (admin): tutti job con log.
	import { goto } from '$app/navigation';
	import * as Card from '$lib/components/ui/card';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as Field from '$lib/components/ui/field';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import JobLog from '$lib/components/app/job-log.svelte';
	import DownloadIcon from '@lucide/svelte/icons/download';
	import { api, errorMessage, formatBytes, type Job } from '$lib/api';
	import { session } from '$lib/session.svelte';
	import { toast } from 'svelte-sonner';
	import type { SiteDetail } from './types';

	let { detail }: { detail: SiteDetail } = $props();
	type ExportRow = { token: string; file: string; size: number; created_at: number; expires_at: number };
	const id = $derived(detail.site.id);
	let exports = $state<ExportRow[]>([]);
	let job = $state<{ id: number; title: string; after?: (j: Job) => void } | null>(null);
	let form = $state<'clone' | 'template' | null>(null);
	let cloneDomain = $state('');
	let tplName = $state('');
	let tplTitle = $state('');

	async function loadExports() {
		exports = await api.get<ExportRow[]>(`/api/sites/${id}/exports`);
	}

	$effect(() => {
		loadExports();
	});

	async function run(path: string, body: unknown, title: string, after?: (j: Job) => void) {
		try {
			const r = await api.post<{ job: number }>(path, body);
			form = null;
			job = { id: r.job, title, after };
		} catch (e) {
			toast.error(errorMessage(e));
		}
	}

	const date = (s: number) => new Date(s * 1000).toLocaleString('it-IT', { dateStyle: 'short', timeStyle: 'short' });
</script>

<Card.Root>
	<Card.Header>
		<Card.Title>Copie ed esportazioni</Card.Title>
		<Card.Description>L'esportazione è un tar.gz con file, dump e LEGGIMI, senza password; resta scaricabile per 7 giorni.</Card.Description>
	</Card.Header>
	<Card.Content class="flex flex-col gap-2">
		{#each exports as e (e.token)}
			<div class="flex items-center justify-between gap-2 text-sm">
				<a class="flex items-center gap-1 truncate underline-offset-4 hover:underline" href={`/api/exports/${e.token}`} download>
					<DownloadIcon class="shrink-0" />
					{e.file}
				</a>
				<span class="text-muted-foreground shrink-0">{formatBytes(e.size)} · scade {date(e.expires_at)}</span>
			</div>
		{:else}
			<p class="text-muted-foreground text-sm">Nessuna esportazione recente.</p>
		{/each}
	</Card.Content>
	<Card.Footer class="flex flex-wrap gap-2">
		<Button variant="outline" onclick={() => run(`/api/sites/${id}/export`, undefined, 'Esportazione', () => loadExports())}>Esporta sito</Button>
		{#if session.user?.is_admin}
			<Button variant="outline" onclick={() => (form = 'clone')}>Clona</Button>
			<Button variant="outline" onclick={() => (form = 'template')}>Salva come template</Button>
		{/if}
	</Card.Footer>
</Card.Root>

<Dialog.Root open={form !== null} onOpenChange={(o) => !o && (form = null)}>
	<Dialog.Content>
		{#if form === 'clone'}
			<Dialog.Header>
				<Dialog.Title>Clona {detail.domains[0]?.hostname}</Dialog.Title>
				<Dialog.Description>File e database copiati su un nuovo dominio; la copia nasce protetta da password.</Dialog.Description>
			</Dialog.Header>
			<form onsubmit={(e) => (e.preventDefault(), run(`/api/sites/${id}/clone`, { domain: cloneDomain }, 'Clonazione', (j) => j.status === 'done' && j.site_id && goto(`/sites/${j.site_id}`)))}>
				<Field.FieldGroup>
					<Field.Field>
						<Field.FieldLabel for="clone-domain">Nuovo dominio</Field.FieldLabel>
						<Input id="clone-domain" placeholder="prove.esempio.it" bind:value={cloneDomain} required />
					</Field.Field>
					<Dialog.Footer><Button type="submit">Clona</Button></Dialog.Footer>
				</Field.FieldGroup>
			</form>
		{:else if form === 'template'}
			<Dialog.Header>
				<Dialog.Title>Salva come template</Dialog.Title>
				<Dialog.Description>File e database diventano un template snapshot per nuovi siti "chiavi in mano".</Dialog.Description>
			</Dialog.Header>
			<form onsubmit={(e) => (e.preventDefault(), run(`/api/sites/${id}/template`, { name: tplName, title: tplTitle }, 'Salvataggio del template'))}>
				<Field.FieldGroup>
					<Field.Field>
						<Field.FieldLabel for="tpl-name">Identificativo</Field.FieldLabel>
						<Input id="tpl-name" placeholder="negozio_base" pattern="[a-z][a-z0-9_]*" bind:value={tplName} required />
						<Field.FieldDescription>Minuscole, cifre e _: diventa il nome della cartella in templates/.</Field.FieldDescription>
					</Field.Field>
					<Field.Field>
						<Field.FieldLabel for="tpl-title">Nome mostrato</Field.FieldLabel>
						<Input id="tpl-title" bind:value={tplTitle} required />
					</Field.Field>
					<Dialog.Footer><Button type="submit">Salva</Button></Dialog.Footer>
				</Field.FieldGroup>
			</form>
		{/if}
	</Dialog.Content>
</Dialog.Root>

<Dialog.Root open={job !== null} onOpenChange={(o) => !o && (job = null)}>
	<Dialog.Content>
		<Dialog.Header><Dialog.Title>{job?.title}</Dialog.Title></Dialog.Header>
		{#if job}
			<JobLog id={job.id} ondone={job.after} />
		{/if}
	</Dialog.Content>
</Dialog.Root>
