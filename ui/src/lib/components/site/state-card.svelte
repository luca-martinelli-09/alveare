<script lang="ts">
	import * as Card from '$lib/components/ui/card';
	import * as Field from '$lib/components/ui/field';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import { Button } from '$lib/components/ui/button';
	import { Switch } from '$lib/components/ui/switch';
	import { api, errorMessage } from '$lib/api';
	import { toast } from 'svelte-sonner';
	import type { SiteDetail } from './types';

	let { detail, onchange }: { detail: SiteDetail; onchange: () => void } = $props();
	let busy = $state(false);
	const suspended = $derived(detail.site.status === 'suspended');

	async function update(body: Record<string, boolean>, done: string) {
		busy = true;
		try {
			await api.patch(`/api/sites/${detail.site.id}`, body);
			toast.success(done);
			onchange();
		} catch (e) {
			toast.error(errorMessage(e));
		} finally {
			busy = false;
		}
	}
</script>

<Card.Root>
	<Card.Header>
		<Card.Title>Stato</Card.Title>
		<Card.Description>Sospensione e protezione delle copie di prova.</Card.Description>
	</Card.Header>
	<Card.Content>
		<Field.FieldGroup>
			<Field.Field orientation="horizontal">
				<Field.FieldContent>
					<Field.FieldLabel for="staging">Protezione dello staging</Field.FieldLabel>
					<Field.FieldDescription>Password per i visitatori e niente indicizzazione nei motori di ricerca.</Field.FieldDescription>
				</Field.FieldContent>
				<Switch
					id="staging"
					checked={detail.site.staging}
					disabled={busy || suspended}
					onCheckedChange={(on) => update({ staging: on }, on ? 'Protezione attivata' : 'Protezione tolta')}
				/>
			</Field.Field>
			{#if detail.staging}
				<p class="text-sm">
					Utente <code>{detail.staging.user}</code>, password <code class="select-all">{detail.staging.password}</code>
				</p>
			{/if}
		</Field.FieldGroup>
	</Card.Content>
	<Card.Footer>
		{#if suspended}
			<Button disabled={busy} onclick={() => update({ suspended: false }, 'Sito riattivato')}>Riattiva il sito</Button>
		{:else}
			<AlertDialog.Root>
				<AlertDialog.Trigger>
					{#snippet child({ props })}
						<Button variant="outline" disabled={busy} {...props}>Sospendi il sito</Button>
					{/snippet}
				</AlertDialog.Trigger>
				<AlertDialog.Content>
					<AlertDialog.Header>
						<AlertDialog.Title>Sospendere {detail.domains[0]?.hostname}?</AlertDialog.Title>
						<AlertDialog.Description>
							I visitatori vedono una pagina di manutenzione e PHP si ferma. File e database restano intatti.
						</AlertDialog.Description>
					</AlertDialog.Header>
					<AlertDialog.Footer>
						<AlertDialog.Cancel>Annulla</AlertDialog.Cancel>
						<AlertDialog.Action onclick={() => update({ suspended: true }, 'Sito sospeso')}>Sospendi</AlertDialog.Action>
					</AlertDialog.Footer>
				</AlertDialog.Content>
			</AlertDialog.Root>
		{/if}
	</Card.Footer>
</Card.Root>
