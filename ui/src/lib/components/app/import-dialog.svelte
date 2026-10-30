<script lang="ts">
	// Importa un sito esistente: archivio dei file e dump caricati a blocchi nell'area import.
	import * as Dialog from '$lib/components/ui/dialog';
	import * as Field from '$lib/components/ui/field';
	import * as Select from '$lib/components/ui/select';
	import * as Alert from '$lib/components/ui/alert';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Progress } from '$lib/components/ui/progress';
	import JobLog from '$lib/components/app/job-log.svelte';
	import UploadIcon from '@lucide/svelte/icons/upload';
	import { api, errorMessage, formatBytes } from '$lib/api';
	import { uploadTo } from '$lib/upload';

	let { ondone }: { ondone: () => void } = $props();
	type ImportFile = { name: string; size: number; modified: number };
	let open = $state(false);
	let files = $state<ImportFile[]>([]);
	let progress = $state<number | null>(null);
	let domain = $state('');
	let title = $state('');
	let archive = $state('');
	let dump = $state('');
	let error = $state('');
	let job = $state<number | null>(null);
	const archives = $derived(files.filter((f) => /\.(zip|tar\.gz|tgz)$/i.test(f.name)));
	const dumps = $derived(files.filter((f) => /\.sql(\.gz)?$/i.test(f.name)));

	async function load() {
		files = await api.get<ImportFile[]>('/api/imports');
	}

	async function upload(e: Event) {
		const input = e.currentTarget as HTMLInputElement;
		error = '';
		try {
			for (const f of Array.from(input.files ?? [])) {
				progress = 0;
				await uploadTo('/api/imports/uploads', f.name, f, (p) => (progress = p * 100));
			}
		} catch (err) {
			error = errorMessage(err);
		} finally {
			progress = null;
			input.value = '';
			await load();
		}
	}

	async function start(e: SubmitEvent) {
		e.preventDefault();
		error = '';
		try {
			job = (await api.post<{ job: number }>('/api/sites/import', { domain, title, archive, dump })).job;
		} catch (err) {
			error = errorMessage(err);
		}
	}

	function setOpen(next: boolean) {
		open = next;
		if (next) load();
		else {
			if (job) ondone();
			job = null;
		}
	}
</script>

<Dialog.Root bind:open={() => open, setOpen}>
	<Dialog.Trigger>
		{#snippet child({ props })}
			<Button variant="outline" {...props}>
				<UploadIcon data-icon="inline-start" />
				Importa sito
			</Button>
		{/snippet}
	</Dialog.Trigger>
	<Dialog.Content class="sm:max-w-lg">
		<Dialog.Header>
			<Dialog.Title>Importa un sito esistente</Dialog.Title>
			<Dialog.Description>
				Carica l'archivio dei file (zip o tar.gz) e il dump del database (.sql o .sql.gz). Il prefisso delle tabelle si legge dal
				vecchio wp-config.php; se il dominio cambia parte il search-replace.
			</Dialog.Description>
		</Dialog.Header>
		{#if job}
			<JobLog id={job} />
		{:else}
			<form onsubmit={start}>
				<Field.FieldGroup>
					{#if error}
						<Alert.Root variant="destructive"><Alert.Description>{error}</Alert.Description></Alert.Root>
					{/if}
					<Field.Field>
						<Field.FieldLabel for="import-upload">Carica file</Field.FieldLabel>
						<Input id="import-upload" type="file" multiple accept=".zip,.gz,.tgz,.sql" disabled={progress !== null} onchange={upload} />
						{#if progress !== null}
							<Progress value={progress} />
						{/if}
						<Field.FieldDescription>
							{files.length ? files.map((f) => `${f.name} (${formatBytes(f.size)})`).join(', ') : 'Area import vuota.'}
						</Field.FieldDescription>
					</Field.Field>
					<Field.Field>
						<Field.FieldLabel>Archivio dei file</Field.FieldLabel>
						<Select.Root type="single" bind:value={archive}>
							<Select.Trigger>{archive || 'Scegli…'}</Select.Trigger>
							<Select.Content>
								<Select.Group>
									{#each archives as f (f.name)}
										<Select.Item value={f.name}>{f.name}</Select.Item>
									{/each}
								</Select.Group>
							</Select.Content>
						</Select.Root>
					</Field.Field>
					<Field.Field>
						<Field.FieldLabel>Dump del database</Field.FieldLabel>
						<Select.Root type="single" bind:value={dump}>
							<Select.Trigger>{dump || 'Scegli…'}</Select.Trigger>
							<Select.Content>
								<Select.Group>
									{#each dumps as f (f.name)}
										<Select.Item value={f.name}>{f.name}</Select.Item>
									{/each}
								</Select.Group>
							</Select.Content>
						</Select.Root>
					</Field.Field>
					<Field.Field>
						<Field.FieldLabel for="import-domain">Dominio sul pannello</Field.FieldLabel>
						<Input id="import-domain" placeholder="prove.esempio.it" bind:value={domain} required />
						<Field.FieldDescription>Per le prove usa un hostname temporaneo: il search-replace lo sistema.</Field.FieldDescription>
					</Field.Field>
					<Field.Field>
						<Field.FieldLabel for="import-title">Nome del sito</Field.FieldLabel>
						<Input id="import-title" bind:value={title} required />
					</Field.Field>
					<Dialog.Footer>
						<Button variant="primary" type="submit" disabled={!archive || !dump || progress !== null}>Importa</Button>
					</Dialog.Footer>
				</Field.FieldGroup>
			</form>
		{/if}
	</Dialog.Content>
</Dialog.Root>
