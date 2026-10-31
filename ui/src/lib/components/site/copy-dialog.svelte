<script lang="ts">
	// "Copia in un altro sito": file o cartella in una cartella di un sito (anche lo stesso), come job.
	import * as Dialog from '$lib/components/ui/dialog';
	import * as Field from '$lib/components/ui/field';
	import * as Select from '$lib/components/ui/select';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Spinner } from '$lib/components/ui/spinner';
	import JobLog from '$lib/components/app/job-log.svelte';
	import { api, errorMessage, type Job } from '$lib/api';
	import { sites, loadSites } from '$lib/sites.svelte';
	import { toast } from 'svelte-sonner';

	/** `path`: percorso della voce da copiare; `dir`: cartella aperta, proposta come arrivo. */
	let { siteId, path, dir, open = $bindable(false), ondone }: { siteId: number; path: string; dir: string; open?: boolean; ondone: (sameSite: boolean) => void } = $props();

	let target = $state('');
	let to = $state('');
	let starting = $state(false);
	let job = $state<number | null>(null);

	$effect(() => {
		if (!open) return;
		[target, to, job] = [String(siteId), dir, null];
		if (!sites.list) loadSites().catch((e) => toast.error(errorMessage(e)));
	});

	const name = $derived(path.split('/').pop() ?? path);
	const label = (id: string) => sites.list?.find((s) => String(s.id) === id)?.domain ?? `sito ${id}`;
	const choices = $derived((sites.list ?? []).filter((s) => s.status === 'active'));

	async function start(e: SubmitEvent) {
		e.preventDefault();
		starting = true;
		try {
			job = (await api.post<{ job: number }>(`/api/sites/${siteId}/files/copy`, { paths: [path], site: Number(target), to: to.trim() })).job;
		} catch (err) {
			toast.error(errorMessage(err));
		} finally {
			starting = false;
		}
	}

	function finished(j: Job) {
		if (j.status !== 'done') return;
		toast.success(`${name} copiato in ${label(target)}`);
		ondone(Number(target) === siteId);
	}
</script>

<Dialog.Root bind:open>
	<Dialog.Content class="sm:max-w-lg">
		<Dialog.Header>
			<Dialog.Title>Copia {name}</Dialog.Title>
			<Dialog.Description>In un altro sito o in un'altra cartella di questo. Se lì c'è già qualcosa con lo stesso nome non si tocca niente.</Dialog.Description>
		</Dialog.Header>
		{#if job}
			{#key job}<JobLog id={job} ondone={finished} />{/key}
			<Dialog.Footer><Button variant="ghost" onclick={() => (open = false)}>Chiudi</Button></Dialog.Footer>
		{:else}
			<form onsubmit={start} class="flex flex-col gap-6">
				<Field.FieldGroup>
					<Field.Field>
						<Field.FieldLabel>Sito</Field.FieldLabel>
						<Select.Root type="single" bind:value={target}>
							<Select.Trigger>{label(target)}</Select.Trigger>
							<Select.Content>
								<Select.Group>
									{#each choices as s (s.id)}
										<Select.Item value={String(s.id)}>{s.domain ?? s.slug}{s.id === siteId ? ' (questo sito)' : ''}</Select.Item>
									{/each}
								</Select.Group>
							</Select.Content>
						</Select.Root>
					</Field.Field>
					<Field.Field>
						<Field.FieldLabel for="copy-to">Cartella di arrivo</Field.FieldLabel>
						<Input id="copy-to" bind:value={to} class="font-mono" placeholder="public/wp-content/uploads" autocomplete="off" spellcheck={false} />
						<Field.FieldDescription>Vuota = radice del sito. La cartella si crea se non c'è.</Field.FieldDescription>
					</Field.Field>
				</Field.FieldGroup>
				<Dialog.Footer>
					<Button variant="ghost" type="button" onclick={() => (open = false)}>Annulla</Button>
					<Button type="submit" disabled={starting || !target}>
						{#if starting}<Spinner data-icon="inline-start" />{/if}
						Copia
					</Button>
				</Dialog.Footer>
			</form>
		{/if}
	</Dialog.Content>
</Dialog.Root>
