<script lang="ts">
	import { goto } from '$app/navigation';
	import * as Table from '$lib/components/ui/table';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as Field from '$lib/components/ui/field';
	import * as Select from '$lib/components/ui/select';
	import * as Alert from '$lib/components/ui/alert';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import PageHeader from '$lib/components/app/page-header.svelte';
	import { api, errorMessage } from '$lib/api';
	import PlusIcon from '@lucide/svelte/icons/plus';

	type Template = { id: string; name: string; kind: string; snapshot: boolean; steps: number };
	const kinds: Record<string, string> = { wordpress: 'WordPress', php: 'PHP', laravel: 'Laravel' };

	let templates = $state<Template[] | null>(null);
	let open = $state(false);
	let name = $state('');
	let id = $state('');
	let idEdited = $state(false);
	let from = $state('');
	let kind = $state('wordpress');
	let error = $state('');

	$effect(() => {
		api.get<Template[]>('/api/templates').then((t) => (templates = t));
	});

	// L'identificativo è il nome della cartella: lo si propone dal nome, si può cambiare.
	const slugify = (s: string) =>
		s.normalize('NFD').replace(/[̀-ͯ]/g, '').toLowerCase().replace(/[^a-z0-9]+/g, '_').replace(/^_+|_+$/g, '').slice(0, 40);
	$effect(() => {
		if (!idEdited) id = slugify(name);
	});

	async function create(e: SubmitEvent) {
		e.preventDefault();
		error = '';
		try {
			await api.post('/api/templates', { id, name, ...(from ? { from } : { kind }) });
			await goto(`/templates/${id}`);
		} catch (err) {
			error = errorMessage(err);
		}
	}
</script>

<PageHeader title="Template" description="Il punto di partenza dei siti nuovi: file, dump facoltativo e comandi WP-CLI dopo la creazione.">
	{#snippet actions()}
		<Dialog.Root bind:open>
			<Dialog.Trigger>
				{#snippet child({ props })}
					<Button variant="primary" {...props}><PlusIcon data-icon="inline-start" /> Nuovo template</Button>
				{/snippet}
			</Dialog.Trigger>
			<Dialog.Content>
				<Dialog.Header>
					<Dialog.Title>Nuovo template</Dialog.Title>
					<Dialog.Description>Parti da zero o da una copia. Per uno snapshot di un sito esistente usa "Salva come template" nella pagina del sito.</Dialog.Description>
				</Dialog.Header>
				<form onsubmit={create}>
					<Field.FieldGroup>
						{#if error}<Alert.Root variant="destructive"><Alert.Description>{error}</Alert.Description></Alert.Root>{/if}
						<Field.Field>
							<Field.FieldLabel for="tpl-name">Nome</Field.FieldLabel>
							<Input id="tpl-name" bind:value={name} placeholder="Vetrina associazione" required />
						</Field.Field>
						<Field.Field>
							<Field.FieldLabel for="tpl-id">Identificativo</Field.FieldLabel>
							<Input id="tpl-id" bind:value={id} oninput={() => (idEdited = true)} pattern="[a-z0-9_]+" class="font-mono" required />
							<Field.FieldDescription>Nome della cartella in templates/: minuscole, cifre e _.</Field.FieldDescription>
						</Field.Field>
						<Field.Field>
							<Field.FieldLabel>Parti da</Field.FieldLabel>
							<Select.Root type="single" bind:value={from}>
								<Select.Trigger>{from ? `Copia di ${templates?.find((t) => t.id === from)?.name}` : 'Template vuoto'}</Select.Trigger>
								<Select.Content>
									<Select.Group>
										<Select.Item value="" label="Template vuoto">Template vuoto</Select.Item>
										{#each templates ?? [] as t (t.id)}
											<Select.Item value={t.id} label={`Copia di ${t.name}`}>Copia di {t.name}</Select.Item>
										{/each}
									</Select.Group>
								</Select.Content>
							</Select.Root>
						</Field.Field>
						{#if !from}
							<Field.Field>
								<Field.FieldLabel>Tipo</Field.FieldLabel>
								<Select.Root type="single" bind:value={kind}>
									<Select.Trigger>{kinds[kind]}</Select.Trigger>
									<Select.Content>
										<Select.Group>
											{#each Object.entries(kinds) as [value, label] (value)}
												<Select.Item {value} {label}>{label}</Select.Item>
											{/each}
										</Select.Group>
									</Select.Content>
								</Select.Root>
							</Field.Field>
						{/if}
						<Dialog.Footer><Button variant="primary" type="submit">Crea template</Button></Dialog.Footer>
					</Field.FieldGroup>
				</form>
			</Dialog.Content>
		</Dialog.Root>
	{/snippet}
</PageHeader>

{#if templates === null}
	<Skeleton class="h-40 w-full" />
{:else}
	<div class="rounded-lg border">
		<Table.Root>
			<Table.Header>
				<Table.Row>
					<Table.Head class="pl-4">Template</Table.Head>
					<Table.Head class="hidden sm:table-cell">Tipo</Table.Head>
					<Table.Head>Origine</Table.Head>
					<Table.Head class="hidden pr-4 text-right md:table-cell">Comandi dopo la creazione</Table.Head>
				</Table.Row>
			</Table.Header>
			<Table.Body>
				{#each templates as t (t.id)}
					<Table.Row class="group cursor-pointer" onclick={() => goto(`/templates/${t.id}`)}>
						<Table.Cell class="pl-4">
							<a href={`/templates/${t.id}`} class="flex flex-col" onclick={(e) => e.stopPropagation()}>
								<span class="font-medium group-hover:underline">{t.name}</span>
								<span class="text-muted-foreground font-mono text-xs">{t.id}</span>
							</a>
						</Table.Cell>
						<Table.Cell class="hidden sm:table-cell">{kinds[t.kind] ?? t.kind}</Table.Cell>
						<Table.Cell>
							<Badge variant={t.snapshot ? 'secondary' : 'outline'}>{t.snapshot ? 'Snapshot di un sito' : 'Ricetta'}</Badge>
						</Table.Cell>
						<Table.Cell class="text-muted-foreground hidden pr-4 text-right md:table-cell">{t.steps || '–'}</Table.Cell>
					</Table.Row>
				{/each}
			</Table.Body>
		</Table.Root>
	</div>
{/if}
