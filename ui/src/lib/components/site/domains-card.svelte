<script lang="ts">
	import * as Card from '$lib/components/ui/card';
	import * as Field from '$lib/components/ui/field';
	import * as Select from '$lib/components/ui/select';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import XIcon from '@lucide/svelte/icons/x';
	import { api, errorMessage } from '$lib/api';
	import { session } from '$lib/session.svelte';
	import { toast } from 'svelte-sonner';
	import type { SiteDetail } from './types';

	let { detail, onchange }: { detail: SiteDetail; onchange: () => void } = $props();
	const kinds = { primary: 'principale', alias: 'alias', redirect: 'redirect' };
	let hostname = $state('');
	let kind = $state<'alias' | 'redirect'>('redirect');
	const base = $derived(`/api/sites/${detail.site.id}/domains`);

	async function add(e: SubmitEvent) {
		e.preventDefault();
		try {
			await api.post(base, { hostname: hostname.trim(), kind });
			toast.success(`${hostname} aggiunto`);
			hostname = '';
			onchange();
		} catch (err) {
			toast.error(errorMessage(err));
		}
	}

	async function remove(h: string) {
		try {
			await api.del(`${base}/${encodeURIComponent(h)}`);
			onchange();
		} catch (err) {
			toast.error(errorMessage(err));
		}
	}
</script>

<Card.Root>
	<Card.Header>
		<Card.Title>Domini</Card.Title>
		<Card.Description>Con un account Cloudflare configurato, hostname e regole vanno sul tunnel da soli; senza, aggiungili a mano verso http://caddy:80.</Card.Description>
	</Card.Header>
	<Card.Content class="flex flex-col gap-2">
		{#each detail.domains as d (d.hostname)}
			<div class="flex items-center justify-between gap-2">
				<a class="truncate underline-offset-4 hover:underline" href={d.url} target="_blank" rel="noreferrer">{d.hostname}</a>
				<div class="flex items-center gap-1">
					<Badge variant="outline">{kinds[d.kind]}</Badge>
					{#if session.user?.is_admin && d.kind !== 'primary'}
						<Button variant="ghost" size="icon-sm" aria-label={`Togli ${d.hostname}`} onclick={() => remove(d.hostname)}>
							<XIcon />
						</Button>
					{/if}
				</div>
			</div>
		{/each}
	</Card.Content>
	{#if session.user?.is_admin}
		<Card.Footer class="flex flex-col gap-3">
			<form class="flex w-full flex-wrap items-end gap-2" onsubmit={add}>
				<Field.Field class="min-w-48 flex-1">
					<Field.FieldLabel for="new-host">Nuovo hostname</Field.FieldLabel>
					<Input id="new-host" bind:value={hostname} placeholder="www.esempio.it" required />
				</Field.Field>
				<Select.Root type="single" bind:value={kind}>
					<Select.Trigger class="w-32" aria-label="Tipo">{kinds[kind]}</Select.Trigger>
					<Select.Content>
						<Select.Group>
							<Select.Item value="redirect">redirect</Select.Item>
							<Select.Item value="alias">alias</Select.Item>
						</Select.Group>
					</Select.Content>
				</Select.Root>
				<Button type="submit">Aggiungi</Button>
			</form>
		</Card.Footer>
	{/if}
</Card.Root>
