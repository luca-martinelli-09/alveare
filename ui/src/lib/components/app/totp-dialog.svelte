<script lang="ts">
	// Spostare il TOTP su un'altra app: password, poi QR e codice della nuova app.
	import QRCode from 'qrcode';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as Field from '$lib/components/ui/field';
	import * as Alert from '$lib/components/ui/alert';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { api, errorMessage } from '$lib/api';
	import { toast } from 'svelte-sonner';

	let { open = $bindable(false) }: { open?: boolean } = $props();
	let password = $state('');
	let code = $state('');
	let setup = $state<{ secret: string; qr: string } | null>(null);
	let error = $state('');

	function reset() {
		password = code = error = '';
		setup = null;
	}

	async function start(e: SubmitEvent) {
		e.preventDefault();
		error = '';
		try {
			const r = await api.post<{ secret: string; otpauth_url: string }>('/api/me/totp', { password });
			setup = { secret: r.secret, qr: await QRCode.toDataURL(r.otpauth_url, { margin: 1, width: 200 }) };
		} catch (err) {
			error = errorMessage(err);
		}
	}

	async function confirm(e: SubmitEvent) {
		e.preventDefault();
		error = '';
		try {
			await api.post('/api/me/totp/confirm', { code });
			toast.success('Fatto: da ora vale il codice della nuova app');
			open = false;
			reset();
		} catch (err) {
			error = errorMessage(err);
		}
	}
</script>

<Dialog.Root bind:open onOpenChange={(o) => !o && reset()}>
	<Dialog.Content class="sm:max-w-md">
		<Dialog.Header>
			<Dialog.Title>Sposta su un'altra app</Dialog.Title>
			<Dialog.Description>
				{setup ? 'Inquadra il codice con la nuova app e scrivi il codice che mostra.' : "Per sicurezza serve la password. Finché non confermi, vale ancora l'app di adesso."}
			</Dialog.Description>
		</Dialog.Header>
		{#if error}<Alert.Root variant="destructive"><Alert.Description>{error}</Alert.Description></Alert.Root>{/if}
		{#if !setup}
			<form onsubmit={start}>
				<Field.FieldGroup>
					<Field.Field>
						<Field.FieldLabel for="totp-pw">Password</Field.FieldLabel>
						<Input id="totp-pw" type="password" autocomplete="current-password" bind:value={password} required />
					</Field.Field>
					<Dialog.Footer>
						<Button variant="ghost" type="button" onclick={() => (open = false)}>Annulla</Button>
						<Button variant="primary" type="submit">Continua</Button>
					</Dialog.Footer>
				</Field.FieldGroup>
			</form>
		{:else}
			<form onsubmit={confirm}>
				<Field.FieldGroup>
					<img src={setup.qr} alt="Codice QR per la nuova app di autenticazione" class="mx-auto size-48 rounded-md border bg-white p-1" />
					<p class="text-muted-foreground text-center text-xs">Oppure inserisci a mano: <code class="text-foreground break-all select-all">{setup.secret}</code></p>
					<Field.Field>
						<Field.FieldLabel for="totp-code">Codice della nuova app</Field.FieldLabel>
						<Input id="totp-code" inputmode="numeric" autocomplete="one-time-code" class="text-center font-mono tracking-widest" bind:value={code} required />
					</Field.Field>
					<Dialog.Footer>
						<Button variant="ghost" type="button" onclick={() => (open = false)}>Annulla</Button>
						<Button variant="primary" type="submit">Conferma</Button>
					</Dialog.Footer>
				</Field.FieldGroup>
			</form>
		{/if}
	</Dialog.Content>
</Dialog.Root>
