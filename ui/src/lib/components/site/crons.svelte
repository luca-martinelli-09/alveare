<script lang="ts">
	import { askConfirm } from '$lib/ask.svelte';
	// Cron personalizzati: WP-CLI, artisan, script PHP del sito o una pagina del sito.
	import * as Card from '$lib/components/ui/card';
	import * as Table from '$lib/components/ui/table';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as Field from '$lib/components/ui/field';
	import * as Select from '$lib/components/ui/select';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import JobLog from '$lib/components/app/job-log.svelte';
	import EllipsisIcon from '@lucide/svelte/icons/ellipsis';
	import { api, errorMessage } from '$lib/api';
	import { toast } from 'svelte-sonner';

	let { id, canManage, space = false }: { id: number; canManage: boolean; space?: boolean } = $props();
	type Cron = { id: number; expression: string; kind: string; command: string; timeout_s: number; active: boolean };
	type Run = { id: number; started_at: string; finished_at: string | null; ok: boolean | null; output: string };
	let crons = $state<{ cron: Cron; last: Run | null }[]>([]);
	let creating = $state(false);
	let form = $state({ expression: '*/15 * * * *', kind: 'wp', command: '', timeout_s: 600 });
	// Stesso dialogo per un cron nuovo e per uno da modificare (id).
	let editing = $state<number | null>(null);

	function openNew() {
		[editing, form, creating] = [null, { expression: '*/15 * * * *', kind: 'wp', command: '', timeout_s: 600 }, true];
	}

	function openEdit(c: Cron) {
		[editing, form, creating] = [c.id, { expression: c.expression, kind: c.kind, command: c.command, timeout_s: c.timeout_s }, true];
	}
	let job = $state<number | null>(null);
	let history = $state<{ cron: Cron; runs: Run[] } | null>(null);
	// Uno spazio chiama qualunque URL pubblico; un sito solo le sue pagine.
	const kinds = $derived({ wp: 'WP-CLI', artisan: 'artisan', php: 'Script PHP', http: space ? 'Chiamata HTTP' : 'Pagina (HTTP)' });
	const placeholders = $derived({ wp: 'cache flush', artisan: 'schedule:run', php: space ? 'scripts/sincronizza.php' : 'public/cron.php', http: space ? 'https://api.esempio.it/sync' : 'https://esempio.it/wp-cron.php' });

	async function load() {
		crons = await api.get<typeof crons>(`/api/sites/${id}/cron`);
	}

	$effect(() => {
		load();
	});

	async function create(e: SubmitEvent) {
		e.preventDefault();
		try {
			if (editing === null) await api.post(`/api/sites/${id}/cron`, form);
			else await api.put(`/api/sites/${id}/cron/${editing}`, form);
			toast.success(editing === null ? 'Cron creato' : 'Cron aggiornato');
			creating = false;
			form.command = '';
			await load();
		} catch (err) {
			toast.error(errorMessage(err));
		}
	}

	async function act(fn: () => Promise<unknown>) {
		try {
			await fn();
			await load();
		} catch (err) {
			toast.error(errorMessage(err));
		}
	}

	async function showRuns(cron: Cron) {
		history = { cron, runs: await api.get<Run[]>(`/api/sites/${id}/cron/${cron.id}/runs`) };
	}
</script>

<div class="flex flex-col gap-4 pt-4">
	<Card.Root>
		<Card.Header>
			<Card.Title>Cron personalizzati</Card.Title>
			<Card.Description>
				Espressione cron in ora di Roma. Il cron di WordPress gira già da solo ogni 5 minuti. Un cron non parte se il precedente è ancora in corso; se fallisce
				arriva un avviso.
			</Card.Description>
		</Card.Header>
		<Card.Content>
			<Table.Root>
				<Table.Body>
					{#each crons as { cron, last } (cron.id)}
						<Table.Row>
							<Table.Cell><code>{cron.expression}</code></Table.Cell>
							<Table.Cell><Badge variant="outline">{kinds[cron.kind as keyof typeof kinds]}</Badge></Table.Cell>
							<Table.Cell class="max-w-sm truncate font-mono text-xs">{cron.command}</Table.Cell>
							<Table.Cell>
								{#if !cron.active}<Badge variant="secondary">In pausa</Badge>
								{:else if last?.ok === false}<Badge variant="destructive">Fallito</Badge>
								{:else if last?.ok}<Badge variant="secondary">Riuscito</Badge>{/if}
							</Table.Cell>
							<Table.Cell class="text-right">
								<DropdownMenu.Root>
									<DropdownMenu.Trigger>
										{#snippet child({ props })}
											<Button variant="ghost" size="icon-sm" {...props} aria-label="Azioni"><EllipsisIcon /></Button>
										{/snippet}
									</DropdownMenu.Trigger>
									<DropdownMenu.Content align="end">
										<DropdownMenu.Group>
											<DropdownMenu.Item onclick={() => showRuns(cron)}>Ultime esecuzioni</DropdownMenu.Item>
											{#if canManage}
												<DropdownMenu.Item onclick={() => openEdit(cron)}>Modifica</DropdownMenu.Item>
												<DropdownMenu.Item onclick={() => act(async () => (job = (await api.post<{ job: number }>(`/api/sites/${id}/cron/${cron.id}/run`)).job))}>Esegui ora</DropdownMenu.Item>
												<DropdownMenu.Item onclick={() => act(() => api.patch(`/api/sites/${id}/cron/${cron.id}`, { active: !cron.active }))}>
													{cron.active ? 'Metti in pausa' : 'Riattiva'}
												</DropdownMenu.Item>
												<DropdownMenu.Item variant="destructive" onclick={async () => (await askConfirm({ title: 'Eliminare il cron?', confirmLabel: 'Elimina', destructive: true })) && act(() => api.del(`/api/sites/${id}/cron/${cron.id}`))}>Elimina</DropdownMenu.Item>
											{/if}
										</DropdownMenu.Group>
									</DropdownMenu.Content>
								</DropdownMenu.Root>
							</Table.Cell>
						</Table.Row>
					{:else}
						<Table.Row><Table.Cell class="text-muted-foreground">Nessun cron personalizzato.</Table.Cell></Table.Row>
					{/each}
				</Table.Body>
			</Table.Root>
		</Card.Content>
		{#if canManage}
			<Card.Footer><Button onclick={openNew}>Nuovo cron</Button></Card.Footer>
		{/if}
	</Card.Root>
</div>

<Dialog.Root bind:open={creating}>
	<Dialog.Content>
		<Dialog.Header><Dialog.Title>{editing === null ? 'Nuovo cron' : 'Modifica cron'}</Dialog.Title></Dialog.Header>
		<form onsubmit={create}>
			<Field.FieldGroup>
				<Field.Field>
					<Field.FieldLabel for="cron-expr">Quando</Field.FieldLabel>
					<Input id="cron-expr" bind:value={form.expression} required />
					<Field.FieldDescription>minuto ora giorno mese giorno-della-settimana, es. 0 3 * * * = ogni notte alle 3</Field.FieldDescription>
				</Field.Field>
				<Field.Field>
					<Field.FieldLabel>Tipo</Field.FieldLabel>
					<Select.Root type="single" bind:value={form.kind}>
						<Select.Trigger>{kinds[form.kind as keyof typeof kinds]}</Select.Trigger>
						<Select.Content>
							<Select.Group>
								{#each Object.entries(kinds) as [v, label] (v)}
									<Select.Item value={v}>{label}</Select.Item>
								{/each}
							</Select.Group>
						</Select.Content>
					</Select.Root>
				</Field.Field>
				<Field.Field>
					<Field.FieldLabel for="cron-cmd">Comando</Field.FieldLabel>
					<Input id="cron-cmd" class="font-mono" placeholder={placeholders[form.kind as keyof typeof placeholders]} bind:value={form.command} required />
				</Field.Field>
				<Field.Field>
					<Field.FieldLabel for="cron-timeout">Timeout (secondi)</Field.FieldLabel>
					<Input id="cron-timeout" type="number" min="1" max="3600" bind:value={form.timeout_s} />
				</Field.Field>
				<Dialog.Footer><Button variant="primary" type="submit">{editing === null ? 'Crea' : 'Salva'}</Button></Dialog.Footer>
			</Field.FieldGroup>
		</form>
	</Dialog.Content>
</Dialog.Root>

<Dialog.Root open={history !== null} onOpenChange={(o) => !o && (history = null)}>
	<Dialog.Content class="sm:max-w-3xl">
		<Dialog.Header><Dialog.Title class="font-mono text-sm">{history?.cron.command}</Dialog.Title></Dialog.Header>
		<div class="flex max-h-[60vh] flex-col gap-3 overflow-auto">
			{#each history?.runs ?? [] as r (r.id)}
				<div class="flex flex-col gap-1">
					<div class="flex items-center gap-2 text-sm">
						<Badge variant={r.ok === false ? 'destructive' : r.ok ? 'secondary' : 'outline'}>{r.ok === null ? 'in corso' : r.ok ? 'ok' : 'fallito'}</Badge>
						{new Date(r.started_at).toLocaleString('it-IT')}
					</div>
					{#if r.output}<pre class="bg-muted rounded p-2 text-xs whitespace-pre-wrap">{r.output}</pre>{/if}
				</div>
			{:else}
				<p class="text-muted-foreground text-sm">Ancora nessuna esecuzione.</p>
			{/each}
		</div>
	</Dialog.Content>
</Dialog.Root>

<Dialog.Root open={job !== null} onOpenChange={(o) => !o && ((job = null), load())}>
	<Dialog.Content>
		<Dialog.Header><Dialog.Title>Esecuzione</Dialog.Title></Dialog.Header>
		{#if job}<JobLog id={job} />{/if}
	</Dialog.Content>
</Dialog.Root>
