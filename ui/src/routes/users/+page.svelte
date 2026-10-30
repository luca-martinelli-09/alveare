<script lang="ts">
	import PageHeader from '$lib/components/app/page-header.svelte';
	import * as Table from '$lib/components/ui/table';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as Field from '$lib/components/ui/field';
	import * as Alert from '$lib/components/ui/alert';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Checkbox } from '$lib/components/ui/checkbox';
	import { api, errorMessage, type User } from '$lib/api';
	import { goto } from '$app/navigation';
	import { toast } from 'svelte-sonner';
	import PlusIcon from '@lucide/svelte/icons/plus';


	let users = $state<User[]>([]);
	let creating = $state(false);
	let email = $state('');
	let name = $state('');
	let isAdmin = $state(false);
	let invite = $state(true);
	let canInvite = $state(false);
	let totp = $state(true);
	let created = $state<{ email: string; password: string | null } | null>(null);

	async function load() {
		users = await api.get<User[]>('/api/users');
	}

	$effect(() => {
		load();
		api.get<{ email: boolean; totp: boolean }>('/api/auth/options').then((o) => ((canInvite = o.email), (totp = o.totp)));
	});

	async function reinvite(u: User) {
		try {
			await api.post(`/api/users/${u.id}/invite`);
			toast.success(`Nuovo invito mandato a ${u.email}`);
		} catch (e) {
			toast.error(errorMessage(e));
		}
	}

	async function create(e: SubmitEvent) {
		e.preventDefault();
		try {
			const r = await api.post<{ temporary_password?: string }>('/api/users', { email, name, is_admin: isAdmin, invite: canInvite && invite });
			created = { email, password: r.temporary_password ?? null };
			email = name = '';
			isAdmin = false;
			await load();
		} catch (err) {
			toast.error(errorMessage(err));
		}
	}




</script>

<PageHeader title="Utenti" description="Gli admin vedono tutto; gli altri solo i siti assegnati, da gestore o in sola lettura.">
	{#snippet actions()}
	<Dialog.Root bind:open={() => creating, (o) => ((creating = o), o || (created = null))}>
		<Dialog.Trigger>
			{#snippet child({ props })}
				<Button variant="primary" {...props}><PlusIcon data-icon="inline-start" /> Nuovo utente</Button>
			{/snippet}
		</Dialog.Trigger>
		<Dialog.Content>
			<Dialog.Header>
				<Dialog.Title>Nuovo utente</Dialog.Title>
				<Dialog.Description>Al primo accesso cambierà la password{totp ? ' e attiverà il TOTP' : ''}.</Dialog.Description>
			</Dialog.Header>
			{#if created && created.password === null}
				<Alert.Root>
					<Alert.Title>Invito mandato a {created.email}</Alert.Title>
					<Alert.Description>Il link vale 72 ore: {totp ? 'password e TOTP li sceglie' : 'la password la sceglie'} chi lo riceve.</Alert.Description>
				</Alert.Root>
			{:else if created}
				<Alert.Root>
					<Alert.Title>Password temporanea per {created.email}</Alert.Title>
					<Alert.Description>
						<code class="text-base">{created.password}</code>
						<p>Comunicala di persona o con un canale sicuro: non verrà più mostrata.</p>
					</Alert.Description>
				</Alert.Root>
			{:else}
				<form onsubmit={create}>
					<Field.FieldGroup>
						<Field.Field>
							<Field.FieldLabel for="email">Email</Field.FieldLabel>
							<Input id="email" type="email" bind:value={email} required />
						</Field.Field>
						<Field.Field>
							<Field.FieldLabel for="name">Nome</Field.FieldLabel>
							<Input id="name" bind:value={name} required />
						</Field.Field>
						{#if canInvite}
							<Field.Field orientation="horizontal">
								<Checkbox id="invite" bind:checked={invite} />
								<Field.FieldLabel for="invite">Invita via email (altrimenti password temporanea)</Field.FieldLabel>
							</Field.Field>
						{/if}
						<Field.Field orientation="horizontal">
							<Checkbox id="admin" bind:checked={isAdmin} />
							<Field.FieldLabel for="admin">Amministratore (tutti i siti e le impostazioni)</Field.FieldLabel>
						</Field.Field>
						<Button variant="primary" type="submit">Crea</Button>
					</Field.FieldGroup>
				</form>
			{/if}
		</Dialog.Content>
	</Dialog.Root>
	{/snippet}
</PageHeader>

<div class="rounded-lg border">
<Table.Root>
	<Table.Header>
		<Table.Row>
			<Table.Head>Nome</Table.Head>
			<Table.Head>Email</Table.Head>
			<Table.Head>Ruolo</Table.Head>
			<Table.Head></Table.Head>
		</Table.Row>
	</Table.Header>
	<Table.Body>
		{#each users as u (u.id)}
			<Table.Row class="group cursor-pointer" onclick={() => goto(`/users/${u.id}`)}>
				<Table.Cell class="font-medium"><a href={`/users/${u.id}`} class="group-hover:underline" onclick={(e) => e.stopPropagation()}>{u.name}</a></Table.Cell>
				<Table.Cell>{u.email}</Table.Cell>
				<Table.Cell>
					{#if u.is_admin}<Badge>Admin</Badge>{:else}<Badge variant="outline">Per sito</Badge>{/if}
					{#if u.status === 'invited'}<Badge variant="secondary">invitato</Badge>{/if}
					{#if u.status === 'blocked'}<Badge variant="destructive">bloccato</Badge>{/if}
				</Table.Cell>
				<Table.Cell class="text-right">
					{#if u.status === 'invited' && canInvite}<Button variant="ghost" size="sm" onclick={(e: MouseEvent) => (e.stopPropagation(), reinvite(u))}>Reinvia invito</Button>{/if}
				</Table.Cell>
			</Table.Row>
		{/each}
	</Table.Body>
</Table.Root>
</div>
