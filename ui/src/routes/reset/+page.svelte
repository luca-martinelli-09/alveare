<script lang="ts">
	import AuthShell from '$lib/components/app/auth-shell.svelte';
	// Nuova password dal link dell'email: serve anche il codice TOTP, l'email da sola non basta.
	import { goto } from '$app/navigation';
	import * as Card from '$lib/components/ui/card';
	import * as Field from '$lib/components/ui/field';
	import * as Alert from '$lib/components/ui/alert';
	import { Input } from '$lib/components/ui/input';
	import { Button } from '$lib/components/ui/button';
	import { api, errorMessage, type Stage } from '$lib/api';
	import { loadSession } from '$lib/session.svelte';

	let password = $state('');
	let code = $state('');
	let totp = $state(true);
	$effect(() => {
		api.get<{ totp: boolean }>('/api/auth/options').then((o) => (totp = o.totp)).catch(() => {});
	});
	let error = $state('');
	let busy = $state(false);

	async function submit(e: SubmitEvent) {
		e.preventDefault();
		busy = true;
		error = '';
		try {
			const token = window.location.hash.slice(1);
			const r = await api.post<{ stage: Stage }>('/api/auth/reset/confirm', { token, password, code });
			history.replaceState(null, '', '/reset');
			await loadSession();
			await goto(r.stage === 'full' ? '/' : '/login');
		} catch (err) {
			error = errorMessage(err);
		} finally {
			busy = false;
		}
	}
</script>

<AuthShell>
	<Card.Root>
		<Card.Header>
			<Card.Title>Reimposta la password</Card.Title>
			<Card.Description>Scegli la nuova password{totp ? " e conferma con il codice dell'app di autenticazione" : ''}.</Card.Description>
		</Card.Header>
		<Card.Content>
			<form onsubmit={submit}>
				<Field.FieldGroup>
					{#if error}<Alert.Root variant="destructive"><Alert.Description>{error}</Alert.Description></Alert.Root>{/if}
					<Field.Field>
						<Field.FieldLabel for="pw">Nuova password</Field.FieldLabel>
						<Input id="pw" type="password" autocomplete="new-password" minlength={12} bind:value={password} required />
						<Field.FieldDescription>Almeno 12 caratteri.</Field.FieldDescription>
					</Field.Field>
					{#if totp}
						<Field.Field>
							<Field.FieldLabel for="code">Codice TOTP</Field.FieldLabel>
							<Input id="code" inputmode="numeric" autocomplete="one-time-code" bind:value={code} />
						</Field.Field>
					{/if}
					<Button type="submit" disabled={busy}>Salva e accedi</Button>
				</Field.FieldGroup>
			</form>
		</Card.Content>
	</Card.Root>
</AuthShell>
