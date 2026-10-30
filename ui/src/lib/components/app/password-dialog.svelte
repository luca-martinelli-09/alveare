<script lang="ts">
	// Cambio della propria password: quella attuale, la nuova due volte.
	import * as Dialog from '$lib/components/ui/dialog';
	import * as Field from '$lib/components/ui/field';
	import * as Alert from '$lib/components/ui/alert';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { api, errorMessage } from '$lib/api';
	import { toast } from 'svelte-sonner';

	let { open = $bindable(false) }: { open?: boolean } = $props();
	let current = $state('');
	let next = $state('');
	let again = $state('');
	let error = $state('');

	async function submit(e: SubmitEvent) {
		e.preventDefault();
		error = '';
		if (next !== again) {
			error = 'Le due password nuove non coincidono.';
			return;
		}
		try {
			await api.post('/api/me/password', { current, new: next });
			toast.success('Password cambiata: le altre sessioni sono state chiuse');
			open = false;
			current = next = again = '';
		} catch (err) {
			error = errorMessage(err);
		}
	}
</script>

<Dialog.Root bind:open>
	<Dialog.Content class="sm:max-w-md">
		<Dialog.Header>
			<Dialog.Title>Cambia password</Dialog.Title>
			<Dialog.Description>Almeno 12 caratteri. Le sessioni sugli altri dispositivi vengono chiuse.</Dialog.Description>
		</Dialog.Header>
		<form onsubmit={submit}>
			<Field.FieldGroup>
				{#if error}<Alert.Root variant="destructive"><Alert.Description>{error}</Alert.Description></Alert.Root>{/if}
				<Field.Field>
					<Field.FieldLabel for="pw-current">Password attuale</Field.FieldLabel>
					<Input id="pw-current" type="password" autocomplete="current-password" bind:value={current} required />
				</Field.Field>
				<Field.Field>
					<Field.FieldLabel for="pw-new">Nuova password</Field.FieldLabel>
					<Input id="pw-new" type="password" autocomplete="new-password" minlength={12} bind:value={next} required />
				</Field.Field>
				<Field.Field>
					<Field.FieldLabel for="pw-again">Ripeti la nuova password</Field.FieldLabel>
					<Input id="pw-again" type="password" autocomplete="new-password" minlength={12} bind:value={again} required />
				</Field.Field>
				<Dialog.Footer>
					<Button variant="ghost" type="button" onclick={() => (open = false)}>Annulla</Button>
					<Button variant="primary" type="submit">Cambia password</Button>
				</Dialog.Footer>
			</Field.FieldGroup>
		</form>
	</Dialog.Content>
</Dialog.Root>
