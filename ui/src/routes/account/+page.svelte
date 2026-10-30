<script lang="ts">
	import PageHeader from '$lib/components/app/page-header.svelte';
	import * as Card from '$lib/components/ui/card';
	import * as Field from '$lib/components/ui/field';
	import * as Alert from '$lib/components/ui/alert';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { api, errorMessage } from '$lib/api';
	import { session } from '$lib/session.svelte';
	import { toast } from 'svelte-sonner';
	import DownloadIcon from '@lucide/svelte/icons/download';
	import SecurityCards from '$lib/components/app/security-cards.svelte';

	let name = $state('');
	let days = $state(90);
	let token = $state<string | null>(null);
	let refresh = $state(0);

	async function createToken(e: SubmitEvent) {
		e.preventDefault();
		try {
			token = (await api.post<{ token: string }>('/api/me/tokens', { name, days: Number(days) })).token;
			name = '';
			refresh++;
		} catch (err) {
			toast.error(errorMessage(err));
		}
	}
</script>

<PageHeader title="Account" description="Password, secondo fattore, passkey, sessioni e token API." />
<div class="grid gap-4 md:grid-cols-2">
	<Card.Root>
		<Card.Header>
			<Card.Title>Token API</Card.Title>
			<Card.Description>Per script e per la CLI alveare: hanno i tuoi stessi permessi.</Card.Description>
		</Card.Header>
		<Card.Content class="flex flex-col gap-4">
			{#if token}
				<Alert.Root>
					<Alert.Title>Copia il token adesso</Alert.Title>
					<Alert.Description><code class="break-all">{token}</code></Alert.Description>
				</Alert.Root>
			{/if}
			<form onsubmit={createToken}>
				<Field.FieldGroup>
					<Field.Field>
						<Field.FieldLabel for="tname">Nome</Field.FieldLabel>
						<Input id="tname" placeholder="backup del portatile" bind:value={name} required />
					</Field.Field>
					<Field.Field>
						<Field.FieldLabel for="days">Validità (giorni)</Field.FieldLabel>
						<Input id="days" type="number" min={1} max={365} bind:value={days} />
					</Field.Field>
					<Button type="submit">Crea token</Button>
				</Field.FieldGroup>
			</form>
		</Card.Content>
	</Card.Root>
	{#if session.user?.is_admin}
		<Card.Root>
			<Card.Header>
				<Card.Title>Kit di ripristino</Card.Title>
				<Card.Description>
					Chiavi e istruzioni per ricostruire il server da zero. Conservalo fuori dal server, per esempio nel
					password manager; riscaricalo quando una chiave cambia.
				</Card.Description>
			</Card.Header>
			<Card.Footer>
				<Button href="/api/admin/recovery-kit" variant="outline">
					<DownloadIcon data-icon="inline-start" />
					Scarica il kit
				</Button>
			</Card.Footer>
		</Card.Root>
	{/if}
	<SecurityCards {refresh} />
</div>
