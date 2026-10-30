<script lang="ts">
	import * as Table from '$lib/components/ui/table';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import * as Alert from '$lib/components/ui/alert';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import * as Field from '$lib/components/ui/field';
	import { Input } from '$lib/components/ui/input';
	import JobLog from '$lib/components/app/job-log.svelte';
	import SnapshotBrowser from './snapshot-browser.svelte';
	import { session } from '$lib/session.svelte';
	import { api, errorMessage, formatBytes } from '$lib/api';
	import { toast } from 'svelte-sonner';

	let { id, canManage }: { id: number; canManage: boolean } = $props();
	type Run = { id: number; started_at: string; status: string; snapshot_id: string | null; bytes_added: number | null; total_bytes: number | null; error: string | null };

	type RestoreTest = { at: string; ok: boolean; detail: string } | null;
	let enabled = $state(true);
	let runs = $state<Run[]>([]);
	let restoreTest = $state<RestoreTest>(null);
	let browsing = $state<string | null>(null);
	let newDomainFor = $state<string | null>(null);
	let newDomain = $state('');
	let job = $state<{ id: number; title: string } | null>(null);

	async function load() {
		const r = await api.get<{ enabled: boolean; runs: Run[]; restore_test: RestoreTest }>(`/api/sites/${id}/backups`);
		enabled = r.enabled;
		runs = r.runs;
		restoreTest = r.restore_test;
	}

	$effect(() => {
		load();
	});

	async function start(title: string, fn: () => Promise<{ job: number }>) {
		try {
			job = { id: (await fn()).job, title };
		} catch (e) {
			toast.error(errorMessage(e));
		}
	}

	const backupNow = () => start('Backup', () => api.post(`/api/sites/${id}/backups`));

	function restore(snapshot: string, mode: 'full' | 'files' | 'database') {
		const what = { full: 'file e database', files: 'solo i file', database: 'solo il database' }[mode];
		if (!confirm(`Ripristinare ${what} dallo snapshot ${snapshot.slice(0, 8)}? Prima viene fatto un backup dello stato attuale.`)) return;
		start('Ripristino', () => api.post(`/api/sites/${id}/backups/${snapshot}/restore`, { mode }));
	}

	function restorePath(snapshot: string, path: string) {
		if (!confirm(`Ripristinare ${path} dallo snapshot ${snapshot.slice(0, 8)}? La versione attuale va nel cestino.`)) return;
		browsing = null;
		start('Ripristino di ' + path, () => api.post(`/api/sites/${id}/backups/${snapshot}/restore`, { mode: 'path', path }));
	}

	function restoreNewDomain(e: SubmitEvent) {
		e.preventDefault();
		const snapshot = newDomainFor!;
		newDomainFor = null;
		start('Ripristino su ' + newDomain, () => api.post(`/api/sites/${id}/backups/${snapshot}/restore`, { mode: 'new_domain', domain: newDomain }));
	}
</script>

<div class="flex flex-col gap-4 pt-4">
	{#if !enabled}
		<Alert.Root>
			<Alert.Title>Backup non configurati</Alert.Title>
			<Alert.Description>Imposta RESTIC_REPOSITORY e RESTIC_PASSWORD nel .env del pannello.</Alert.Description>
		</Alert.Root>
	{:else if canManage}
		<div><Button onclick={backupNow}>Backup adesso</Button></div>
	{/if}
	{#if restoreTest}
		<Alert.Root variant={restoreTest.ok ? 'default' : 'destructive'}>
			<Alert.Title>Ultimo restore di prova: {restoreTest.ok ? 'riuscito' : 'fallito'} ({new Date(restoreTest.at).toLocaleDateString('it-IT')})</Alert.Title>
			<Alert.Description>{restoreTest.detail}</Alert.Description>
		</Alert.Root>
	{/if}
	<Table.Root>
		<Table.Header>
			<Table.Row>
				<Table.Head>Quando</Table.Head>
				<Table.Head>Esito</Table.Head>
				<Table.Head>Snapshot</Table.Head>
				<Table.Head class="text-right">Nuovi dati</Table.Head>
				<Table.Head class="text-right">Totale</Table.Head>
				<Table.Head></Table.Head>
			</Table.Row>
		</Table.Header>
		<Table.Body>
			{#each runs as r (r.id)}
				<Table.Row>
					<Table.Cell class="whitespace-nowrap">{new Date(r.started_at).toLocaleString('it-IT')}</Table.Cell>
					<Table.Cell>
						<Badge variant={r.status === 'done' ? 'secondary' : r.status === 'failed' ? 'destructive' : 'outline'}>
							{{ done: 'Riuscito', failed: 'Fallito', running: 'In corso' }[r.status] ?? r.status}
						</Badge>
						{#if r.error}<span class="text-muted-foreground ml-2 text-xs">{r.error}</span>{/if}
					</Table.Cell>
					<Table.Cell><code>{r.snapshot_id?.slice(0, 8) ?? '–'}</code></Table.Cell>
					<Table.Cell class="text-right">{formatBytes(r.bytes_added)}</Table.Cell>
					<Table.Cell class="text-right">{formatBytes(r.total_bytes)}</Table.Cell>
					<Table.Cell class="text-right">
						{#if canManage && r.snapshot_id}
							<DropdownMenu.Root>
								<DropdownMenu.Trigger>
									{#snippet child({ props })}
										<Button variant="outline" size="sm" {...props}>Ripristina</Button>
									{/snippet}
								</DropdownMenu.Trigger>
								<DropdownMenu.Content align="end">
									<DropdownMenu.Group>
										<DropdownMenu.Item onclick={() => restore(r.snapshot_id!, 'full')}>File e database</DropdownMenu.Item>
										<DropdownMenu.Item onclick={() => restore(r.snapshot_id!, 'files')}>Solo file</DropdownMenu.Item>
										<DropdownMenu.Item onclick={() => restore(r.snapshot_id!, 'database')}>Solo database</DropdownMenu.Item>
										<DropdownMenu.Item onclick={() => (browsing = r.snapshot_id)}>Un file o una cartella…</DropdownMenu.Item>
										{#if session.user?.is_admin}
											<DropdownMenu.Item onclick={() => (newDomainFor = r.snapshot_id)}>Su un nuovo dominio…</DropdownMenu.Item>
										{/if}
									</DropdownMenu.Group>
								</DropdownMenu.Content>
							</DropdownMenu.Root>
						{/if}
					</Table.Cell>
				</Table.Row>
			{/each}
		</Table.Body>
	</Table.Root>
</div>

<Dialog.Root open={job !== null} onOpenChange={(o) => !o && ((job = null), load())}>
	<Dialog.Content>
		<Dialog.Header><Dialog.Title>{job?.title}</Dialog.Title></Dialog.Header>
		{#if job}<JobLog id={job.id} />{/if}
	</Dialog.Content>
</Dialog.Root>

<Dialog.Root open={browsing !== null} onOpenChange={(o) => !o && (browsing = null)}>
	<Dialog.Content class="sm:max-w-2xl">
		<Dialog.Header>
			<Dialog.Title>Snapshot {browsing?.slice(0, 8)}</Dialog.Title>
			<Dialog.Description>Scegli un file o una cartella da ripristinare al suo posto.</Dialog.Description>
		</Dialog.Header>
		{#if browsing}
			<SnapshotBrowser {id} snapshot={browsing} onrestore={(p) => restorePath(browsing!, p)} />
		{/if}
	</Dialog.Content>
</Dialog.Root>

<Dialog.Root open={newDomainFor !== null} onOpenChange={(o) => !o && (newDomainFor = null)}>
	<Dialog.Content>
		<Dialog.Header>
			<Dialog.Title>Ripristina su un nuovo dominio</Dialog.Title>
			<Dialog.Description>Lo snapshot diventa un sito nuovo, protetto da password: il sito in produzione non si tocca.</Dialog.Description>
		</Dialog.Header>
		<form onsubmit={restoreNewDomain}>
			<Field.FieldGroup>
				<Field.Field>
					<Field.FieldLabel for="restore-domain">Dominio</Field.FieldLabel>
					<Input id="restore-domain" placeholder="verifica.esempio.it" bind:value={newDomain} required />
				</Field.Field>
				<Dialog.Footer><Button type="submit">Ripristina</Button></Dialog.Footer>
			</Field.FieldGroup>
		</form>
	</Dialog.Content>
</Dialog.Root>
