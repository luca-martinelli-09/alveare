<script lang="ts">
	import * as Table from '$lib/components/ui/table';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as Field from '$lib/components/ui/field';
	import * as Select from '$lib/components/ui/select';
	import * as Alert from '$lib/components/ui/alert';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Checkbox } from '$lib/components/ui/checkbox';
	import { api, errorMessage, type SiteRow, type User } from '$lib/api';
	import { toast } from 'svelte-sonner';
	import PlusIcon from '@lucide/svelte/icons/plus';

	type Assignment = { site_id: number; role: 'manager' | 'readonly' };

	let users = $state<User[]>([]);
	let sites = $state<SiteRow[]>([]);
	let creating = $state(false);
	let email = $state('');
	let name = $state('');
	let isAdmin = $state(false);
	let invite = $state(true);
	let canInvite = $state(false);
	let created = $state<{ email: string; password: string | null } | null>(null);
	let editing = $state<User | null>(null);
	let assignments = $state<Assignment[]>([]);

	async function load() {
		[users, sites] = await Promise.all([api.get<User[]>('/api/users'), api.get<SiteRow[]>('/api/sites')]);
	}

	$effect(() => {
		load();
		api.get<{ email: boolean }>('/api/auth/options').then((o) => (canInvite = o.email));
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

	async function edit(u: User) {
		editing = u;
		assignments = await api.get<Assignment[]>(`/api/users/${u.id}/sites`);
	}

	const roleOf = (siteId: number) => assignments.find((a) => a.site_id === siteId)?.role ?? 'none';

	async function setRole(siteId: number, role: string) {
		if (!editing) return;
		try {
			if (role === 'none') await api.del(`/api/users/${editing.id}/sites/${siteId}`);
			else await api.put(`/api/users/${editing.id}/sites/${siteId}`, { role });
			assignments = await api.get<Assignment[]>(`/api/users/${editing.id}/sites`);
		} catch (e) {
			toast.error(errorMessage(e));
		}
	}

	const roles: Record<string, string> = { none: 'Nessun accesso', manager: 'Gestore', readonly: 'Sola lettura' };
</script>

<div class="flex items-center justify-between">
	<h1 class="text-2xl font-semibold">Utenti</h1>
	<Dialog.Root bind:open={() => creating, (o) => ((creating = o), o || (created = null))}>
		<Dialog.Trigger>
			{#snippet child({ props })}
				<Button {...props}><PlusIcon data-icon="inline-start" /> Nuovo utente</Button>
			{/snippet}
		</Dialog.Trigger>
		<Dialog.Content>
			<Dialog.Header>
				<Dialog.Title>Nuovo utente</Dialog.Title>
				<Dialog.Description>Al primo accesso cambierà la password e attiverà il TOTP.</Dialog.Description>
			</Dialog.Header>
			{#if created && created.password === null}
				<Alert.Root>
					<Alert.Title>Invito mandato a {created.email}</Alert.Title>
					<Alert.Description>Il link vale 72 ore: password e TOTP li sceglie chi lo riceve.</Alert.Description>
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
						<Button type="submit">Crea</Button>
					</Field.FieldGroup>
				</form>
			{/if}
		</Dialog.Content>
	</Dialog.Root>
</div>

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
			<Table.Row>
				<Table.Cell class="font-medium">{u.name}</Table.Cell>
				<Table.Cell>{u.email}</Table.Cell>
				<Table.Cell>
					{#if u.is_admin}<Badge>Admin</Badge>{:else}<Badge variant="outline">Per sito</Badge>{/if}
					{#if u.status === 'invited'}<Badge variant="secondary">invitato</Badge>{/if}
					{#if u.status === 'blocked'}<Badge variant="destructive">bloccato</Badge>{/if}
				</Table.Cell>
				<Table.Cell class="text-right">
					{#if u.status === 'invited' && canInvite}<Button variant="ghost" size="sm" onclick={() => reinvite(u)}>Reinvia invito</Button>{/if}
					{#if !u.is_admin}<Button variant="outline" size="sm" onclick={() => edit(u)}>Siti</Button>{/if}
				</Table.Cell>
			</Table.Row>
		{/each}
	</Table.Body>
</Table.Root>

<Dialog.Root open={editing !== null} onOpenChange={(o) => !o && (editing = null)}>
	<Dialog.Content>
		<Dialog.Header>
			<Dialog.Title>Siti di {editing?.name}</Dialog.Title>
			<Dialog.Description>Il ruolo vale per quel sito: gestore su uno, sola lettura su un altro.</Dialog.Description>
		</Dialog.Header>
		<Field.FieldGroup>
			{#each sites as s (s.id)}
				<Field.Field orientation="horizontal">
					<Field.FieldLabel class="flex-1">{s.domain}</Field.FieldLabel>
					<Select.Root type="single" value={roleOf(s.id)} onValueChange={(v) => setRole(s.id, v)}>
						<Select.Trigger class="w-44">{roles[roleOf(s.id)]}</Select.Trigger>
						<Select.Content>
							<Select.Group>
								{#each Object.entries(roles) as [value, label] (value)}
									<Select.Item {value} {label}>{label}</Select.Item>
								{/each}
							</Select.Group>
						</Select.Content>
					</Select.Root>
				</Field.Field>
			{/each}
		</Field.FieldGroup>
	</Dialog.Content>
</Dialog.Root>
