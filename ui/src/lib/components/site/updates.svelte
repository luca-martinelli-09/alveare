<script lang="ts">
	// Vista aggiornamenti di WordPress: versioni, regola notturna, aggiornamento con backup prima.
	import * as Card from '$lib/components/ui/card';
	import * as Table from '$lib/components/ui/table';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as ToggleGroup from '$lib/components/ui/toggle-group';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Checkbox } from '$lib/components/ui/checkbox';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import JobLog from '$lib/components/app/job-log.svelte';
	import { api, errorMessage } from '$lib/api';
	import { toast } from 'svelte-sonner';

	let { id, canManage }: { id: number; canManage: boolean } = $props();
	type Item = { name: string; status: string; version: string; update_version: string | null };
	type Status = { core: string; core_update: string | null; plugins: Item[]; themes: Item[] };
	let status = $state<Status | null>(null);
	let rule = $state('security');
	let error = $state('');
	let job = $state<number | null>(null);
	let core = $state(false);
	let plugins = $state<string[]>([]);
	let themes = $state<string[]>([]);
	const rules = { auto: 'Automatico', security: 'Solo sicurezza', manual: 'Manuale' };

	async function load() {
		status = null;
		error = '';
		try {
			const r = await api.get<{ status: Status; update_rule: string }>(`/api/sites/${id}/wordpress`);
			status = r.status;
			rule = r.update_rule;
			core = false;
			plugins = [];
			themes = [];
		} catch (e) {
			error = errorMessage(e);
		}
	}

	$effect(() => {
		load();
	});

	async function setRule(v: string) {
		try {
			await api.patch(`/api/sites/${id}`, { update_rule: v });
			rule = v;
			toast.success(`Regola: ${rules[v as keyof typeof rules]}`);
		} catch (e) {
			toast.error(errorMessage(e));
		}
	}

	async function update(all: boolean) {
		const body = all
			? { core: !!status?.core_update, plugins: 'all', themes: 'all' }
			: { core, plugins: plugins.length ? { names: plugins } : 'none', themes: themes.length ? { names: themes } : 'none' };
		try {
			job = (await api.post<{ job: number }>(`/api/sites/${id}/wordpress/update`, body)).job;
		} catch (e) {
			toast.error(errorMessage(e));
		}
	}

	const toggle = (list: string[], name: string, on: boolean) => (on ? [...list, name] : list.filter((n) => n !== name));
	const pending = $derived(status ? (status.core_update ? 1 : 0) + [...status.plugins, ...status.themes].filter((i) => i.update_version).length : 0);
	const selected = $derived(core || plugins.length > 0 || themes.length > 0);
</script>

{#snippet itemsTable(title: string, items: Item[], selection: string[], set: (v: string[]) => void)}
	<Card.Root>
		<Card.Header><Card.Title>{title}</Card.Title></Card.Header>
		<Card.Content>
			<Table.Root>
				<Table.Body>
					{#each items as i (i.name)}
						<Table.Row>
							<Table.Cell class="w-8">
								{#if canManage && i.update_version}
									<Checkbox aria-label={`Aggiorna ${i.name}`} checked={selection.includes(i.name)} onCheckedChange={(on) => set(toggle(selection, i.name, on === true))} />
								{/if}
							</Table.Cell>
							<Table.Cell>{i.name}</Table.Cell>
							<Table.Cell class="text-muted-foreground">{i.status}</Table.Cell>
							<Table.Cell>{i.version}</Table.Cell>
							<Table.Cell>{#if i.update_version}<Badge>→ {i.update_version}</Badge>{/if}</Table.Cell>
						</Table.Row>
					{/each}
				</Table.Body>
			</Table.Root>
		</Card.Content>
	</Card.Root>
{/snippet}

<div class="flex flex-col gap-4 pt-4">
	{#if error}
		<p class="text-destructive text-sm">{error}</p>
	{:else if !status}
		<Skeleton class="h-64 w-full" />
	{:else}
		<Card.Root>
			<Card.Header>
				<Card.Title>WordPress {status.core}</Card.Title>
				<Card.Description>
					Di notte, dopo il backup, il pannello aggiorna secondo la regola del sito; ogni aggiornamento finisce nell'attività con le versioni prima e dopo.
				</Card.Description>
			</Card.Header>
			<Card.Content class="flex flex-wrap items-center gap-4">
				<ToggleGroup.Root type="single" variant="outline" disabled={!canManage} bind:value={() => rule, (v) => v && v !== rule && setRule(v)}>
					{#each Object.entries(rules) as [v, label] (v)}
						<ToggleGroup.Item value={v}>{label}</ToggleGroup.Item>
					{/each}
				</ToggleGroup.Root>
				{#if status.core_update}
					<label class="flex items-center gap-2 text-sm">
						{#if canManage}<Checkbox checked={core} onCheckedChange={(on) => (core = on === true)} />{/if}
						Core: <Badge>→ {status.core_update}</Badge>
					</label>
				{/if}
			</Card.Content>
			{#if canManage}
				<Card.Footer class="flex gap-2">
					<Button disabled={!selected} onclick={() => update(false)}>Aggiorna i selezionati</Button>
					<Button variant="outline" disabled={pending === 0} onclick={() => update(true)}>Aggiorna tutto ({pending})</Button>
					<Button variant="ghost" onclick={load}>Ricontrolla</Button>
				</Card.Footer>
			{/if}
		</Card.Root>
		{@render itemsTable('Plugin', status.plugins, plugins, (v) => (plugins = v))}
		{@render itemsTable('Temi', status.themes, themes, (v) => (themes = v))}
	{/if}
</div>

<Dialog.Root open={job !== null} onOpenChange={(o) => !o && ((job = null), load())}>
	<Dialog.Content>
		<Dialog.Header><Dialog.Title>Aggiornamento (prima un backup)</Dialog.Title></Dialog.Header>
		{#if job}<JobLog id={job} />{/if}
	</Dialog.Content>
</Dialog.Root>
