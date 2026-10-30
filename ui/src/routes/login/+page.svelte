<script lang="ts">
	import AuthShell from '$lib/components/app/auth-shell.svelte';
	import { goto } from '$app/navigation';
	import QRCode from 'qrcode';
	import * as Card from '$lib/components/ui/card';
	import * as Field from '$lib/components/ui/field';
	import * as Alert from '$lib/components/ui/alert';
	import { Input } from '$lib/components/ui/input';
	import { Button } from '$lib/components/ui/button';
	import { Spinner } from '$lib/components/ui/spinner';
	import { api, errorMessage, type Stage } from '$lib/api';
	import { session, loadSession } from '$lib/session.svelte';
	import { passkeysSupported, usePasskey } from '$lib/passkey';
	import KeyRoundIcon from '@lucide/svelte/icons/key-round';

	let stage = $state<Stage | null>(null);
	let email = $state('');
	let password = $state('');
	let newPassword = $state('');
	let code = $state('');
	let totp = $state<{ secret: string; otpauth_url: string; qr: string } | null>(null);
	let error = $state('');
	let busy = $state(false);
	let options = $state({ email: false, passkeys: false });
	let resetSent = $state(false);

	$effect(() => {
		api.get<typeof options>('/api/auth/options').then((o) => (options = o)).catch(() => {});
	});

	$effect(() => {
		if (session.loaded && session.stage && stage === null) stage = session.stage;
		if (stage === 'full') goto('/');
	});

	async function run(fn: () => Promise<void>) {
		busy = true;
		error = '';
		try {
			await fn();
		} catch (e) {
			error = errorMessage(e);
		} finally {
			busy = false;
		}
	}

	const login = () =>
		run(async () => {
			const r = await api.post<{ stage: Stage }>('/api/auth/login', { email, password });
			if (r.stage === 'full') await loadSession();
			stage = r.stage;
		});

	const withPasskey = () =>
		run(async () => {
			if (!email) throw new Error("Scrivi l'email, poi usa la passkey.");
			const start = await api.post<{ id: string; options: unknown }>('/api/auth/passkey/start', { email });
			const credential = await usePasskey(start.options);
			await api.post('/api/auth/passkey/finish', { id: start.id, credential });
			await loadSession();
			stage = 'full';
		});

	const forgot = () =>
		run(async () => {
			if (!email) throw new Error("Scrivi l'email dell'account.");
			await api.post('/api/auth/reset', { email });
			resetSent = true;
		});

	const setPassword = () =>
		run(async () => {
			const r = await api.post<{ secret: string; otpauth_url: string } | { stage: 'full' }>('/api/auth/setup', { password: newPassword });
			// Con ALVEARE_TOTP=false la password basta: la sessione è già completa.
			if ('stage' in r) {
				await loadSession();
				stage = 'full';
				return;
			}
			totp = { ...r, qr: await QRCode.toDataURL(r.otpauth_url, { margin: 1, width: 200 }) };
		});

	const confirm = () =>
		run(async () => {
			await api.post(stage === 'setup' ? '/api/auth/setup/totp' : '/api/auth/totp', { code });
			await loadSession();
			stage = 'full';
		});
</script>

<AuthShell>
	<Card.Root>
		<Card.Header>
			<Card.Title>Accedi</Card.Title>
			<Card.Description>
				{#if stage === 'setup' && !totp}
					Primo accesso: scegli una nuova password.
				{:else if stage === 'setup'}
					Attiva l'autenticazione a due fattori con un'app TOTP.
				{:else if stage === 'totp'}
					Inserisci il codice dell'app di autenticazione.
				{:else}
					Accedi al pannello.
				{/if}
			</Card.Description>
		</Card.Header>
		<Card.Content class="flex flex-col gap-4">
			{#if error}
				<Alert.Root variant="destructive">
					<Alert.Description>{error}</Alert.Description>
				</Alert.Root>
			{/if}

			{#if stage === null}
				<form onsubmit={(e) => (e.preventDefault(), login())}>
					<Field.FieldGroup>
						<Field.Field>
							<Field.FieldLabel for="email">Email</Field.FieldLabel>
							<Input id="email" type="email" autocomplete="username" bind:value={email} required />
						</Field.Field>
						<Field.Field>
							<Field.FieldLabel for="password">Password</Field.FieldLabel>
							<Input id="password" type="password" autocomplete="current-password" bind:value={password} />
						</Field.Field>
						<Button variant="primary" type="submit" disabled={busy}>
							{#if busy}<Spinner data-icon="inline-start" />{/if}
							Accedi
						</Button>
						{#if options.passkeys && passkeysSupported()}
							<Button type="button" variant="outline" disabled={busy} onclick={withPasskey}>
								<KeyRoundIcon data-icon="inline-start" />
								Accedi con una passkey
							</Button>
						{/if}
						{#if options.email}
							{#if resetSent}
								<Field.FieldDescription>Se l'email è registrata, è partito un link per scegliere una nuova password.</Field.FieldDescription>
							{:else}
								<Button type="button" variant="link" size="sm" disabled={busy} onclick={forgot}>Password dimenticata?</Button>
							{/if}
						{/if}
					</Field.FieldGroup>
				</form>
			{:else if stage === 'setup' && !totp}
				<form onsubmit={(e) => (e.preventDefault(), setPassword())}>
					<Field.FieldGroup>
						<Field.Field>
							<Field.FieldLabel for="new">Nuova password</Field.FieldLabel>
							<Input id="new" type="password" autocomplete="new-password" minlength={12} bind:value={newPassword} required />
							<Field.FieldDescription>Almeno 12 caratteri.</Field.FieldDescription>
						</Field.Field>
						<Button variant="primary" type="submit" disabled={busy}>Continua</Button>
					</Field.FieldGroup>
				</form>
			{:else}
				<form onsubmit={(e) => (e.preventDefault(), confirm())}>
					<Field.FieldGroup>
						{#if totp}
							<img src={totp.qr} alt="Codice QR per l'app di autenticazione" class="mx-auto size-48" />
							<Field.Field>
								<Field.FieldLabel>Oppure inserisci la chiave a mano</Field.FieldLabel>
								<code class="bg-muted rounded p-2 text-xs break-all">{totp.secret}</code>
							</Field.Field>
						{/if}
						<Field.Field>
							<Field.FieldLabel for="code">Codice a 6 cifre</Field.FieldLabel>
							<Input id="code" inputmode="numeric" autocomplete="one-time-code" maxlength={6} bind:value={code} required />
						</Field.Field>
						<Button variant="primary" type="submit" disabled={busy}>
							{#if busy}<Spinner data-icon="inline-start" />{/if}
							Conferma
						</Button>
					</Field.FieldGroup>
				</form>
			{/if}
		</Card.Content>
	</Card.Root>
</AuthShell>
