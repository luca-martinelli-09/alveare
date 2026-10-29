<script lang="ts">
	import * as Table from '$lib/components/ui/table';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import * as Alert from '$lib/components/ui/alert';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import JobLog from '$lib/components/app/job-log.svelte';
	import { api, errorMessage, formatBytes } from '$lib/api';
	import { toast } from 'svelte-sonner';

	let { id, canManage }: { id: number; canManage: boolean } = $props();
	type Run = { id: number; started_at: string; status: string; snapshot_id: string | null; bytes_added: number | null; total_bytes: number | null; error: string | null };

	let enabled = $state(true);
	let runs = $state<Run[]>([]);
	let job = $state<{ id: number; title: string } | null>(null);

	async function load() {
		const r = await api.get<{ enabled: boolean; runs: Run[] }>(`/api/sites/${id}/backups`);
		enabled = r.enabled;
		runs = r.runs;
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
