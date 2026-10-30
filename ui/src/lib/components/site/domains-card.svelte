<script lang="ts">
	import SettingsSection from '$lib/components/app/settings-section.svelte';
	import SettingRow from '$lib/components/app/setting-row.svelte';
	import * as Select from '$lib/components/ui/select';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
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

<SettingsSection title="Domini" description="Con un account Cloudflare configurato, hostname e regole vanno sul tunnel da soli; senza, aggiungili a mano verso http://caddy:80.">
	{#each detail.domains as d (d.hostname)}
		<SettingRow help={{ primary: 'Dominio principale.', alias: 'Alias: stesso sito, stesso indirizzo nella barra.', redirect: 'Rimanda al dominio principale (301).' }[d.kind]}>
			{#snippet label()}<a class="font-medium underline-offset-4 hover:underline" href={d.url} target="_blank" rel="noreferrer">{d.hostname}</a>{/snippet}
			<Badge variant="outline">{kinds[d.kind]}</Badge>
			{#if session.user?.is_admin && d.kind !== 'primary'}
				<Button variant="outline" size="sm" onclick={() => remove(d.hostname)}>Togli</Button>
			{/if}
		</SettingRow>
	{/each}
	{#if session.user?.is_admin}
		<SettingRow label="Aggiungi un hostname" labelFor="new-host" help="Alias o redirect: va sul tunnel da solo.">
			<form class="flex flex-wrap gap-2" onsubmit={add}>
				<Input id="new-host" class="w-52" bind:value={hostname} placeholder="www.esempio.it" required />
				<Select.Root type="single" bind:value={kind}>
					<Select.Trigger class="w-32" aria-label="Tipo">{kinds[kind]}</Select.Trigger>
					<Select.Content>
						<Select.Group>
							<Select.Item value="redirect">redirect</Select.Item>
							<Select.Item value="alias">alias</Select.Item>
						</Select.Group>
					</Select.Content>
				</Select.Root>
				<Button type="submit" variant="outline">Aggiungi</Button>
			</form>
		</SettingRow>
	{/if}
</SettingsSection>
