<script lang="ts">
	import { askConfirm } from '$lib/ask.svelte';
	// App (Laravel e simili): deploy da git e worker long-running.
	import * as Card from '$lib/components/ui/card';
	import * as Table from '$lib/components/ui/table';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as Field from '$lib/components/ui/field';
	import * as Alert from '$lib/components/ui/alert';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import JobLog from '$lib/components/app/job-log.svelte';
	import { api, errorMessage } from '$lib/api';
	import { toast } from 'svelte-sonner';

	let { id, canManage }: { id: number; canManage: boolean } = $props();
	type Worker = { id: number; name: string; command: string; processes: number; memory_mb: number; active: boolean };
	type Proc = { index: number; pid: number | null; uptime_s: number; restarts: number; rss_mb: number | null };
	let repo = $state('');
	let branch = $state('main');
	let publicKey = $state('');
	let job = $state<number | null>(null);
	let workers = $state<{ supervisor: boolean; workers: { worker: Worker; processes: Proc[] }[] }>({ supervisor: true, workers: [] });
	let creating = $state(false);
	let form = $state({ name: 'horizon', command: 'php artisan horizon', processes: 1, memory_mb: 256 });

	async function load() {
		const [d, w] = await Promise.all([api.get<{ repo: string | null; branch: string | null }>(`/api/sites/${id}/deploy`), api.get<typeof workers>(`/api/sites/${id}/workers`)]);
		repo = d.repo ?? '';
		branch = d.branch ?? 'main';
		workers = w;
	}

	$effect(() => {
		if (canManage) load();
	});

	async function act(fn: () => Promise<unknown>, done?: string) {
		try {
			await fn();
			if (done) toast.success(done);
			await load();
		} catch (e) {
			toast.error(errorMessage(e));
		}
	}

	const saveDeploy = (e: SubmitEvent) => (e.preventDefault(), act(() => api.put(`/api/sites/${id}/deploy`, { repo, branch }), 'Repository salvato'));
	const runDeploy = () => act(async () => (job = (await api.post<{ job: number }>(`/api/sites/${id}/deploy`)).job));
	const showKey = () => act(async () => (publicKey = (await api.post<{ public_key: string }>(`/api/sites/${id}/deploy/key`)).public_key));
	const createWorker = (e: SubmitEvent) => (e.preventDefault(), act(async () => {
		await api.post(`/api/sites/${id}/workers`, form);
		creating = false;
	}, 'Worker avviato'));
	const uptime = (s: number) => (s < 120 ? `${s} s` : s < 7200 ? `${Math.round(s / 60)} min` : `${Math.round(s / 3600)} h`);
</script>

<div class="flex flex-col gap-4 pt-4">
	<Card.Root>
		<Card.Header>
			<Card.Title>Deploy da git</Card.Title>
			<Card.Description>
				git fetch e reset sul branch, composer install --no-dev, artisan migrate --force e cache, poi i worker ripartono. Il .env resta quello del pannello.
			</Card.Description>
		</Card.Header>
		<Card.Content class="flex flex-col gap-4">
			<form class="flex flex-wrap items-end gap-2" onsubmit={saveDeploy}>
				<Field.Field class="min-w-72 flex-1">
					<Field.FieldLabel for="repo">Repository</Field.FieldLabel>
					<Input id="repo" class="font-mono" placeholder="git@github.com:ente/app.git" bind:value={repo} required />
				</Field.Field>
				<Field.Field class="w-40">
					<Field.FieldLabel for="branch">Branch</Field.FieldLabel>
					<Input id="branch" class="font-mono" bind:value={branch} required />
				</Field.Field>
				<Button type="submit" variant="outline">Salva</Button>
			</form>
			{#if publicKey}
				<Alert.Root>
					<Alert.Title>Chiave di deploy</Alert.Title>
					<Alert.Description>
						Aggiungila al repository come deploy key in sola lettura:
						<code class="block break-all select-all">{publicKey}</code>
					</Alert.Description>
				</Alert.Root>
			{/if}
		</Card.Content>
		<Card.Footer class="flex gap-2">
			<Button disabled={!repo} onclick={runDeploy}>Deploy adesso</Button>
			<Button variant="ghost" onclick={showKey}>Chiave SSH di deploy</Button>
		</Card.Footer>
	</Card.Root>

	<Card.Root>
		<Card.Header>
			<Card.Title>Worker</Card.Title>
			<Card.Description>Code, Horizon, Reverb: girano nel container php-workers con l'utente del sito, ripartono se si fermano e si fermano oltre la memoria.</Card.Description>
		</Card.Header>
		<Card.Content class="flex flex-col gap-3">
			{#if !workers.supervisor}
				<Alert.Root variant="destructive">
					<Alert.Description>Container php-workers spento: docker compose --profile workers up -d</Alert.Description>
				</Alert.Root>
			{/if}
			<Table.Root>
				<Table.Body>
					{#each workers.workers as { worker: w, processes } (w.id)}
						<Table.Row>
							<Table.Cell class="font-medium">{w.name}</Table.Cell>
							<Table.Cell class="font-mono text-xs">{w.command}</Table.Cell>
							<Table.Cell>
								{#if !w.active}<Badge variant="secondary">fermo</Badge>{/if}
								{#each processes as p (p.index)}
									<Badge variant={p.pid ? 'outline' : 'destructive'} title={`riavvii: ${p.restarts}`}>
										#{p.index + 1} {p.pid ? `${uptime(p.uptime_s)}, ${p.rss_mb ?? '?'} MB` : 'in riavvio'}
									</Badge>
								{/each}
							</Table.Cell>
							<Table.Cell class="text-right whitespace-nowrap">
								<Button variant="ghost" size="sm" onclick={() => act(() => api.post(`/api/sites/${id}/workers/${w.id}/restart`), 'Riavviato')}>Riavvia</Button>
								<Button variant="ghost" size="sm" onclick={() => act(() => api.patch(`/api/sites/${id}/workers/${w.id}`, { active: !w.active }))}>{w.active ? 'Ferma' : 'Avvia'}</Button>
								<Button variant="ghost" size="sm" onclick={async () => (await askConfirm({ title: `Eliminare il worker ${w.name}?`, description: 'Il processo viene fermato.', confirmLabel: 'Elimina', destructive: true })) && act(() => api.del(`/api/sites/${id}/workers/${w.id}`))}>Elimina</Button>
							</Table.Cell>
						</Table.Row>
					{:else}
						<Table.Row><Table.Cell class="text-muted-foreground">Nessun worker.</Table.Cell></Table.Row>
					{/each}
				</Table.Body>
			</Table.Root>
		</Card.Content>
		<Card.Footer><Button variant="outline" onclick={() => (creating = true)}>Nuovo worker</Button></Card.Footer>
	</Card.Root>
</div>

<Dialog.Root bind:open={creating}>
	<Dialog.Content>
		<Dialog.Header><Dialog.Title>Nuovo worker</Dialog.Title></Dialog.Header>
		<form onsubmit={createWorker}>
			<Field.FieldGroup>
				<Field.Field>
					<Field.FieldLabel for="w-name">Nome</Field.FieldLabel>
					<Input id="w-name" pattern="[a-z0-9_-]+" bind:value={form.name} required />
				</Field.Field>
				<Field.Field>
					<Field.FieldLabel for="w-cmd">Comando</Field.FieldLabel>
					<Input id="w-cmd" class="font-mono" bind:value={form.command} required />
					<Field.FieldDescription>Es. php artisan horizon, php artisan queue:work --sleep=3, php artisan reverb:start</Field.FieldDescription>
				</Field.Field>
				<div class="grid grid-cols-2 gap-4">
					<Field.Field>
						<Field.FieldLabel for="w-proc">Processi</Field.FieldLabel>
						<Input id="w-proc" type="number" min="1" max="8" bind:value={form.processes} />
					</Field.Field>
					<Field.Field>
						<Field.FieldLabel for="w-mem">Memoria (MB)</Field.FieldLabel>
						<Input id="w-mem" type="number" min="32" max="4096" bind:value={form.memory_mb} />
					</Field.Field>
				</div>
				<Dialog.Footer><Button variant="primary" type="submit">Avvia</Button></Dialog.Footer>
			</Field.FieldGroup>
		</form>
	</Dialog.Content>
</Dialog.Root>

<Dialog.Root open={job !== null} onOpenChange={(o) => !o && ((job = null), load())}>
	<Dialog.Content class="sm:max-w-3xl">
		<Dialog.Header><Dialog.Title>Deploy</Dialog.Title></Dialog.Header>
		{#if job}<JobLog id={job} />{/if}
	</Dialog.Content>
</Dialog.Root>
