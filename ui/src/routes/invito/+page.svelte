<script lang="ts">
	import AuthShell from '$lib/components/app/auth-shell.svelte';
	// Link d'invito: apre la sessione di primo accesso (password e TOTP) nella pagina di login.
	import { goto } from '$app/navigation';
	import * as Card from '$lib/components/ui/card';
	import * as Alert from '$lib/components/ui/alert';
	import { Spinner } from '$lib/components/ui/spinner';
	import { api, errorMessage } from '$lib/api';
	import { loadSession } from '$lib/session.svelte';

	let error = $state('');

	$effect(() => {
		const token = window.location.hash.slice(1);
		if (!token) {
			error = 'Link incompleto: apri il link dell\'email così com\'è.';
			return;
		}
		api.post('/api/auth/invite', { token })
			.then(async () => {
				history.replaceState(null, '', '/invito');
				await loadSession();
				await goto('/login');
			})
			.catch((e) => (error = errorMessage(e)));
	});
</script>

<AuthShell>
	<Card.Root>
		<Card.Header><Card.Title>Invito ad Alveare</Card.Title></Card.Header>
		<Card.Content>
			{#if error}
				<Alert.Root variant="destructive"><Alert.Description>{error} Chiedi a un admin di mandarne uno nuovo.</Alert.Description></Alert.Root>
			{:else}
				<Spinner />
			{/if}
		</Card.Content>
	</Card.Root>
</AuthShell>
