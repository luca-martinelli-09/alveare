<script lang="ts">
	import { goto } from '$app/navigation';
	import * as Table from '$lib/components/ui/table';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as Field from '$lib/components/ui/field';
	import * as Select from '$lib/components/ui/select';
	import * as Alert from '$lib/components/ui/alert';
	import * as Empty from '$lib/components/ui/empty';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import StatusBadge from '$lib/components/app/status-badge.svelte';
	import JobLog from '$lib/components/app/job-log.svelte';
	import { api, errorMessage, type Alert as AlertT, type SiteRow } from '$lib/api';
	import { session } from '$lib/session.svelte';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import TriangleAlertIcon from '@lucide/svelte/icons/triangle-alert';
	import GlobeIcon from '@lucide/svelte/icons/globe';

	let sites = $state<SiteRow[] | null>(null);
	let alerts = $state<AlertT[]>([]);
	let templates = $state<{ id: string; name: string }[]>([]);
	let open = $state(false);
	let domain = $state('');
	let title = $state('');
	let template = $state('wordpress');
	let error = $state('');
	let job = $state<number | null>(null);

	async function load() {
		[sites, alerts] = await Promise.all([api.get<SiteRow[]>('/api/sites'), api.get<AlertT[]>('/api/alerts')]);
	}

	$effect(() => {
		load();
		api.get<typeof templates>('/api/templates').then((t) => (templates = t));
	});

	async function create(e: SubmitEvent) {
		e.preventDefault();
		error = '';
		try {
			const r = await api.post<{ job: number }>('/api/sites', { domain, title, template });
			job = r.job;
		} catch (err) {
			error = errorMessage(err);
		}
	}

	function closeDialog(next: boolean) {
		open = next;
		if (!next) {
			job = null;
			domain = '';
			title = '';
			load();
		}
	}

	const siteName = (id: number | null) => sites?.find((s) => s.id === id)?.domain ?? 'pannello';
</script>

<div class="flex items-center justify-between">
	<h1 class="text-2xl font-semibold">Siti</h1>
	{#if session.user?.is_admin}
		<Dialog.Root bind:open={() => open, closeDialog}>
			<Dialog.Trigger>
				{#snippet child({ props })}
					<Button {...props}>
						<PlusIcon data-icon="inline-start" />
						Nuovo sito
					</Button>
				{/snippet}
			</Dialog.Trigger>
			<Dialog.Content>
				<Dialog.Header>
					<Dialog.Title>Nuovo sito</Dialog.Title>
					<Dialog.Description>Database, utente, cartella, pool PHP e route vengono creati insieme.</Dialog.Description>
				</Dialog.Header>
				{#if job}
					<JobLog id={job} />
				{:else}
					<form onsubmit={create}>
						<Field.FieldGroup>
							{#if error}
								<Alert.Root variant="destructive"><Alert.Description>{error}</Alert.Description></Alert.Root>
							{/if}
							<Field.Field>
								<Field.FieldLabel for="domain">Dominio</Field.FieldLabel>
								<Input id="domain" placeholder="esempio.it" bind:value={domain} required />
							</Field.Field>
							<Field.Field>
								<Field.FieldLabel for="title">Titolo</Field.FieldLabel>
								<Input id="title" bind:value={title} required />
							</Field.Field>
							<Field.Field>
								<Field.FieldLabel>Template</Field.FieldLabel>
								<Select.Root type="single" bind:value={template}>
									<Select.Trigger>{templates.find((t) => t.id === template)?.name ?? template}</Select.Trigger>
									<Select.Content>
										<Select.Group>
											{#each templates as t (t.id)}
												<Select.Item value={t.id} label={t.name}>{t.name}</Select.Item>
											{/each}
										</Select.Group>
									</Select.Content>
								</Select.Root>
							</Field.Field>
							<Dialog.Footer>
								<Button type="submit">Crea sito</Button>
							</Dialog.Footer>
						</Field.FieldGroup>
					</form>
				{/if}
			</Dialog.Content>
		</Dialog.Root>
	{/if}
</div>

{#each alerts as alert (alert.id)}
	<Alert.Root variant="destructive">
		<TriangleAlertIcon />
		<Alert.Title>{siteName(alert.site_id)} · {alert.kind}</Alert.Title>
		<Alert.Description><pre class="text-xs whitespace-pre-wrap">{alert.message}</pre></Alert.Description>
	</Alert.Root>
{/each}

{#if sites === null}
	<Skeleton class="h-40 w-full" />
{:else if sites.length === 0}
	<Empty.Root>
		<Empty.Header>
			<Empty.Media variant="icon"><GlobeIcon /></Empty.Media>
			<Empty.Title>Nessun sito</Empty.Title>
			<Empty.Description>
				{session.user?.is_admin ? 'Crea il primo sito da un template.' : 'Non hai ancora siti assegnati.'}
			</Empty.Description>
		</Empty.Header>
	</Empty.Root>
{:else}
	<Table.Root>
		<Table.Header>
			<Table.Row>
				<Table.Head>Dominio</Table.Head>
				<Table.Head>Slug</Table.Head>
				<Table.Head>Tipo</Table.Head>
				<Table.Head>Stato</Table.Head>
			</Table.Row>
		</Table.Header>
		<Table.Body>
			{#each sites as site (site.id)}
				<Table.Row class="cursor-pointer" onclick={() => goto(`/sites/${site.id}`)}>
					<Table.Cell class="font-medium">{site.domain}</Table.Cell>
					<Table.Cell class="text-muted-foreground">{site.slug}</Table.Cell>
					<Table.Cell>{site.kind}</Table.Cell>
					<Table.Cell><StatusBadge status={site.status} /></Table.Cell>
				</Table.Row>
			{/each}
		</Table.Body>
	</Table.Root>
{/if}
