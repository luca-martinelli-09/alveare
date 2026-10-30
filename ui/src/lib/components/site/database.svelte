<script lang="ts">
	import * as Card from '$lib/components/ui/card';
	import * as Table from '$lib/components/ui/table';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as Field from '$lib/components/ui/field';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Switch } from '$lib/components/ui/switch';
	import JobLog from '$lib/components/app/job-log.svelte';
	import { api, errorMessage, formatBytes } from '$lib/api';
	import { askConfirm } from '$lib/ask.svelte';
	import Credentials from '$lib/components/app/credentials.svelte';
	import { toast } from 'svelte-sonner';
	import DownloadIcon from '@lucide/svelte/icons/download';
	import ExternalLinkIcon from '@lucide/svelte/icons/external-link';

	let { id }: { id: number } = $props();
	type Overview = { name: string; user: string; host: string; total_bytes: number; tables: { name: string; rows: number; bytes: number }[] };

	let db = $state<Overview | null>(null);
	let importPath = $state('');
	// Un dump è di solito un sito intero (phpMyAdmin non mette DROP TABLE): si parte da un database vuoto.
	let replaceAll = $state(true);
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
			job = (await api.post<{ job: number }>(`/api/sites/${id}/database/import`, { path: importPath, search_replace, replace: replaceAll })).job;
		} catch (err) {
			toast.error(errorMessage(err));
		}
	}

	let newCreds = $state<{ user: string; password: string; database: string } | null>(null);

	async function resetPassword() {
		if (!(await askConfirm({ title: 'Cambiare la password del database?', description: 'Il sito usa subito la nuova password. Altri programmi che usano la vecchia smetteranno di collegarsi.', confirmLabel: 'Cambia' }))) return;
		try {
			newCreds = await api.post<{ user: string; password: string; database: string }>(`/api/sites/${id}/database/password`);
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
						<Field.Field orientation="horizontal">
							<Switch id="replace-all" bind:checked={replaceAll} />
							<Field.Content>
								<Field.FieldLabel for="replace-all">Sostituisci tutto il database</Field.FieldLabel>
								<Field.FieldDescription>
									{replaceAll ? 'Le tabelle attuali si tolgono prima: serve per un sito intero (export di phpMyAdmin compresi).' : 'Le tabelle del dump si aggiungono a quelle che ci sono; se ne esiste già una, l\'import si ferma.'}
								</Field.FieldDescription>
							</Field.Content>
						</Field.Field>
						<Button type="submit" class="self-end">Importa</Button>
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
				<Button variant="outline" onclick={resetPassword}>Cambia password</Button>
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

<Dialog.Root open={newCreds !== null} onOpenChange={(o) => !o && (newCreds = null)}>
	<Dialog.Content class="sm:max-w-xl">
		<Dialog.Header>
			<Dialog.Title>Password del database cambiata</Dialog.Title>
			<Dialog.Description>
				Il .env del sito è già aggiornato. Copiala ora se ti serve per un altro programma: chiusa questa finestra non si rivede.
			</Dialog.Description>
		</Dialog.Header>
		{#if newCreds}
			<p class="text-muted-foreground text-sm">Database <code class="text-foreground">{newCreds.database}</code></p>
			<Credentials id="db" user={newCreds.user} password={newCreds.password} />
		{/if}
		<Dialog.Footer><Button onclick={() => (newCreds = null)}>Fatto</Button></Dialog.Footer>
	</Dialog.Content>
</Dialog.Root>
