<script lang="ts">
	import * as Card from '$lib/components/ui/card';
	import * as Table from '$lib/components/ui/table';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as Field from '$lib/components/ui/field';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import JobLog from '$lib/components/app/job-log.svelte';
	import { api, errorMessage, formatBytes } from '$lib/api';
	import { toast } from 'svelte-sonner';
	import DownloadIcon from '@lucide/svelte/icons/download';
	import ExternalLinkIcon from '@lucide/svelte/icons/external-link';

	let { id }: { id: number } = $props();
	type Overview = { name: string; user: string; host: string; total_bytes: number; tables: { name: string; rows: number; bytes: number }[] };

	let db = $state<Overview | null>(null);
	let importPath = $state('');
	let replaceFrom = $state('');
	let replaceTo = $state('');
	let job = $state<number | null>(null);

	async function load() {
		try {
			db = await api.get<Overview>(`/api/sites/${id}/database`);
		} catch (e) {
			toast.error(errorMessage(e));
		}
	}

	$effect(() => {
		load();
	});

	async function startImport(e: SubmitEvent) {
		e.preventDefault();
		try {
			const search_replace = replaceFrom && replaceTo ? { from: replaceFrom, to: replaceTo } : undefined;
			job = (await api.post<{ job: number }>(`/api/sites/${id}/database/import`, { path: importPath, search_replace })).job;
		} catch (err) {
			toast.error(errorMessage(err));
		}
	}

	async function resetPassword() {
		try {
			await api.post(`/api/sites/${id}/database/password`);
			toast.success('Password cambiata e .env aggiornato');
		} catch (e) {
			toast.error(errorMessage(e));
		}
	}

	async function adminer() {
		try {
			const r = await api.post<{ url: string }>(`/api/sites/${id}/database/adminer`);
			window.open(r.url, '_blank', 'noopener');
		} catch (e) {
			toast.error(errorMessage(e));
		}
	}
</script>

<div class="grid gap-4 pt-4 md:grid-cols-3">
	<Card.Root class="md:col-span-2">
		<Card.Header>
			<Card.Title>{db?.name ?? 'Database'}</Card.Title>
			<Card.Description>{db ? `${formatBytes(db.total_bytes)} · utente ${db.user} su ${db.host}` : ''}</Card.Description>
			<Card.Action class="flex gap-2">
				<Button variant="outline" size="sm" href={`/api/sites/${id}/database/export`}>
					<DownloadIcon data-icon="inline-start" />
					Esporta
				</Button>
				<Button variant="outline" size="sm" onclick={adminer}>
					<ExternalLinkIcon data-icon="inline-start" />
					Adminer
				</Button>
			</Card.Action>
		</Card.Header>
		<Card.Content>
			<Table.Root>
				<Table.Header>
					<Table.Row>
						<Table.Head>Tabella</Table.Head>
						<Table.Head class="text-right">Righe</Table.Head>
						<Table.Head class="text-right">Dimensione</Table.Head>
					</Table.Row>
				</Table.Header>
				<Table.Body>
					{#each db?.tables ?? [] as t (t.name)}
						<Table.Row>
							<Table.Cell><code>{t.name}</code></Table.Cell>
							<Table.Cell class="text-right">{t.rows.toLocaleString('it-IT')}</Table.Cell>
							<Table.Cell class="text-right">{formatBytes(t.bytes)}</Table.Cell>
						</Table.Row>
					{/each}
				</Table.Body>
			</Table.Root>
		</Card.Content>
	</Card.Root>

	<div class="flex flex-col gap-4">
		<Card.Root>
			<Card.Header>
				<Card.Title>Importa un dump</Card.Title>
				<Card.Description>Carica prima il file .sql o .sql.gz dal file manager. Prima dell'import si salva un dump di sicurezza.</Card.Description>
			</Card.Header>
			<Card.Content>
				<form onsubmit={startImport}>
					<Field.FieldGroup>
						<Field.Field>
							<Field.FieldLabel for="dump">Percorso nel sito</Field.FieldLabel>
							<Input id="dump" placeholder="import.sql.gz" bind:value={importPath} required />
						</Field.Field>
						<Field.Field>
							<Field.FieldLabel for="from">Sostituisci dominio (facoltativo)</Field.FieldLabel>
							<Input id="from" placeholder="vecchio.it" bind:value={replaceFrom} />
							<Input placeholder="nuovo.it" bind:value={replaceTo} />
							<Field.FieldDescription>wp search-replace, rispetta i dati serializzati.</Field.FieldDescription>
						</Field.Field>
						<Button type="submit">Importa</Button>
					</Field.FieldGroup>
				</form>
			</Card.Content>
		</Card.Root>
		<Card.Root>
			<Card.Header>
				<Card.Title>Credenziali</Card.Title>
				<Card.Description>Nuova password per l'utente del database, scritta anche nel .env del sito.</Card.Description>
			</Card.Header>
			<Card.Footer>
				<AlertDialog.Root>
					<AlertDialog.Trigger>
						{#snippet child({ props })}
							<Button variant="outline" {...props}>Cambia password</Button>
						{/snippet}
					</AlertDialog.Trigger>
					<AlertDialog.Content>
						<AlertDialog.Header>
							<AlertDialog.Title>Cambiare la password del database?</AlertDialog.Title>
							<AlertDialog.Description>Il sito usa subito la nuova password. Altri programmi che usano la vecchia smetteranno di collegarsi.</AlertDialog.Description>
						</AlertDialog.Header>
						<AlertDialog.Footer>
							<AlertDialog.Cancel>Annulla</AlertDialog.Cancel>
							<AlertDialog.Action onclick={resetPassword}>Cambia</AlertDialog.Action>
						</AlertDialog.Footer>
					</AlertDialog.Content>
				</AlertDialog.Root>
			</Card.Footer>
		</Card.Root>
	</div>
</div>

<Dialog.Root open={job !== null} onOpenChange={(o) => !o && ((job = null), load())}>
	<Dialog.Content>
		<Dialog.Header><Dialog.Title>Import del database</Dialog.Title></Dialog.Header>
		{#if job}<JobLog id={job} />{/if}
	</Dialog.Content>
</Dialog.Root>
