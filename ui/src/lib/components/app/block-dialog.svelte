<script lang="ts">
	// "Blocca IP" dalla riga di un log: durata e ambito (tutti i siti solo per gli admin).
	import * as Dialog from '$lib/components/ui/dialog';
	import * as Field from '$lib/components/ui/field';
	import * as ToggleGroup from '$lib/components/ui/toggle-group';
	import { Button } from '$lib/components/ui/button';
	import { api, errorMessage } from '$lib/api';
	import { session } from '$lib/session.svelte';
	import { toast } from 'svelte-sonner';

	let { siteId, ip = $bindable() }: { siteId: number; ip: string | null } = $props();
	let duration = $state('24h');
	let scope = $state('site');

	async function block() {
		try {
			await api.post(`/api/sites/${siteId}/blocks`, { ip, duration, scope });
			toast.success(`${ip} bloccato`);
			ip = null;
		} catch (e) {
			toast.error(errorMessage(e));
		}
	}
</script>

<Dialog.Root open={ip !== null} onOpenChange={(o) => !o && (ip = null)}>
	<Dialog.Content>
		<Dialog.Header>
			<Dialog.Title>Blocca {ip}</Dialog.Title>
			<Dialog.Description>Caddy risponderà 403 a questo IP; il blocco si toglie da solo alla scadenza.</Dialog.Description>
		</Dialog.Header>
		<Field.FieldGroup>
			<Field.Field>
				<Field.FieldLabel>Durata</Field.FieldLabel>
				<ToggleGroup.Root type="single" variant="outline" bind:value={() => duration, (v) => v && (duration = v)}>
					<ToggleGroup.Item value="1h">1 ora</ToggleGroup.Item>
					<ToggleGroup.Item value="24h">24 ore</ToggleGroup.Item>
					<ToggleGroup.Item value="7d">7 giorni</ToggleGroup.Item>
					<ToggleGroup.Item value="always">Sempre</ToggleGroup.Item>
				</ToggleGroup.Root>
			</Field.Field>
			{#if session.user?.is_admin}
				<Field.Field>
					<Field.FieldLabel>Ambito</Field.FieldLabel>
					<ToggleGroup.Root type="single" variant="outline" bind:value={() => scope, (v) => v && (scope = v)}>
						<ToggleGroup.Item value="site">Solo questo sito</ToggleGroup.Item>
						<ToggleGroup.Item value="all">Tutti i siti</ToggleGroup.Item>
					</ToggleGroup.Root>
				</Field.Field>
			{/if}
		</Field.FieldGroup>
		<Dialog.Footer><Button variant="destructive" onclick={block}>Blocca</Button></Dialog.Footer>
	</Dialog.Content>
</Dialog.Root>
