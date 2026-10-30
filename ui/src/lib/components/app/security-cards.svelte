<script lang="ts">
	// Profilo: passkey, sessioni aperte e token API, con revoca.
	import * as Card from '$lib/components/ui/card';
	import * as Table from '$lib/components/ui/table';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { api, errorMessage } from '$lib/api';
	import { createPasskey, passkeysSupported } from '$lib/passkey';
	import { toast } from 'svelte-sonner';

	let { refresh = 0 }: { refresh?: number } = $props();
	type Passkey = { id: number; name: string; created_at: number; last_used: number | null };
	type Sess = { id: string; created_at: number; last_used: number; ip: string; user_agent: string; current: boolean };
	type Tok = { id: number; name: string; expires_at: number; last_used: number | null };
	let passkeys = $state<{ enabled: boolean; passkeys: Passkey[] }>({ enabled: false, passkeys: [] });
	let sessions = $state<Sess[]>([]);
	let tokens = $state<Tok[]>([]);

	async function load() {
		[passkeys, sessions, tokens] = await Promise.all([api.get<typeof passkeys>('/api/me/passkeys'), api.get<Sess[]>('/api/me/sessions'), api.get<Tok[]>('/api/me/tokens')]);
	}

	$effect(() => {
		refresh;
		load();
	});

	async function act(fn: () => Promise<unknown>, done?: string) {
		try {
			await fn();
			if (done) toast.success(done);
			await load();
		} catch (e) {
			toast.error(errorMessage(e));
		}
	}

	const addPasskey = () =>
		act(async () => {
			const name = prompt('Nome della passkey', 'Portatile');
			if (!name) return;
			const start = await api.post<{ id: string; options: unknown }>('/api/me/passkeys/register/start');
			const credential = await createPasskey(start.options);
			await api.post('/api/me/passkeys/register/finish', { id: start.id, name, credential });
		}, 'Passkey aggiunta');

	const when = (t: number | null) => (t ? new Date(t * 1000).toLocaleString('it-IT', { dateStyle: 'short', timeStyle: 'short' }) : '–');
</script>

<Card.Root>
	<Card.Header>
		<Card.Title>Passkey</Card.Title>
		<Card.Description>Accesso senza password: impronta, volto o PIN del dispositivo valgono come secondo fattore.</Card.Description>
	</Card.Header>
	<Card.Content>
		<Table.Root>
			<Table.Body>
				{#each passkeys.passkeys as p (p.id)}
					<Table.Row>
						<Table.Cell>{p.name}</Table.Cell>
						<Table.Cell class="text-muted-foreground">usata: {when(p.last_used)}</Table.Cell>
						<Table.Cell class="text-right">
							<Button variant="outline" size="sm" onclick={() => confirm(`Togliere ${p.name}?`) && act(() => api.del(`/api/me/passkeys/${p.id}`))}>Togli</Button>
						</Table.Cell>
					</Table.Row>
				{:else}
					<Table.Row><Table.Cell class="text-muted-foreground">Nessuna passkey.</Table.Cell></Table.Row>
				{/each}
			</Table.Body>
		</Table.Root>
	</Card.Content>
	<Card.Footer>
		<Button disabled={!passkeys.enabled || !passkeysSupported()} onclick={addPasskey}>Aggiungi una passkey</Button>
	</Card.Footer>
</Card.Root>

<Card.Root>
	<Card.Header>
		<Card.Title>Sessioni aperte</Card.Title>
		<Card.Description>Scadono dopo 12 ore senza attività.</Card.Description>
	</Card.Header>
	<Card.Content>
		<Table.Root>
			<Table.Body>
				{#each sessions as s (s.id)}
					<Table.Row>
						<Table.Cell class="max-w-48 truncate" title={s.user_agent}>{s.user_agent || 'sconosciuto'}</Table.Cell>
						<Table.Cell><code>{s.ip}</code></Table.Cell>
						<Table.Cell class="text-muted-foreground">{when(s.last_used)}</Table.Cell>
						<Table.Cell class="text-right">
							{#if s.current}<Badge variant="secondary">questa</Badge>
							{:else}<Button variant="outline" size="sm" onclick={() => act(() => api.del(`/api/me/sessions/${s.id}`), 'Sessione chiusa')}>Chiudi</Button>{/if}
						</Table.Cell>
					</Table.Row>
				{/each}
			</Table.Body>
		</Table.Root>
	</Card.Content>
	<Card.Footer>
		<Button variant="outline" disabled={sessions.length < 2} onclick={() => act(() => api.post('/api/me/sessions/others'), 'Altre sessioni chiuse')}>Esci dagli altri dispositivi</Button>
	</Card.Footer>
</Card.Root>

{#if tokens.length}
	<Card.Root>
		<Card.Header><Card.Title>I tuoi token API</Card.Title></Card.Header>
		<Card.Content>
			<Table.Root>
				<Table.Body>
					{#each tokens as t (t.id)}
						<Table.Row>
							<Table.Cell>{t.name}</Table.Cell>
							<Table.Cell class="text-muted-foreground">scade {when(t.expires_at)} · usato {when(t.last_used)}</Table.Cell>
							<Table.Cell class="text-right">
								<Button variant="outline" size="sm" onclick={() => confirm(`Revocare ${t.name}?`) && act(() => api.del(`/api/me/tokens/${t.id}`), 'Token revocato')}>Revoca</Button>
							</Table.Cell>
						</Table.Row>
					{/each}
				</Table.Body>
			</Table.Root>
		</Card.Content>
	</Card.Root>
{/if}
