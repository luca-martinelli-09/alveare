<script lang="ts">
	import SettingsSection from '$lib/components/app/settings-section.svelte';
	import SettingRow from '$lib/components/app/setting-row.svelte';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as Field from '$lib/components/ui/field';
	import * as ToggleGroup from '$lib/components/ui/toggle-group';
	import PlusIcon from '@lucide/svelte/icons/plus';
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
	let adding = $state(false);
	const base = $derived(`/api/sites/${detail.site.id}/domains`);

	async function add(e: SubmitEvent) {
		e.preventDefault();
		try {
			await api.post(base, { hostname: hostname.trim(), kind });
			toast.success(`${hostname} aggiunto`);
			hostname = '';
			adding = false;
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
	{#snippet actions()}
		{#if session.user?.is_admin}
			<Button size="icon-sm" aria-label="Aggiungi un hostname" title="Aggiungi un hostname" onclick={() => (adding = true)}><PlusIcon /></Button>
		{/if}
	{/snippet}
	{#each detail.domains as d (d.hostname)}
		<SettingRow help={{ primary: 'Dominio principale.', alias: 'Alias: stesso sito, stesso indirizzo nella barra.', redirect: 'Rimanda al dominio principale (301).' }[d.kind]}>
			{#snippet label()}<a class="font-medium underline-offset-4 hover:underline" href={d.url} target="_blank" rel="noreferrer">{d.hostname}</a>{/snippet}
			<Badge variant="outline">{kinds[d.kind]}</Badge>
			{#if session.user?.is_admin && d.kind !== 'primary'}
				<Button variant="outline" size="sm" onclick={() => remove(d.hostname)}>Togli</Button>
			{/if}
		</SettingRow>
	{/each}
</SettingsSection>

<Dialog.Root bind:open={adding}>
	<Dialog.Content class="sm:max-w-md">
		<Dialog.Header>
			<Dialog.Title>Aggiungi un hostname</Dialog.Title>
			<Dialog.Description>Va sul tunnel Cloudflare da solo, con le regole della sua zona.</Dialog.Description>
		</Dialog.Header>
		<form onsubmit={add}>
			<Field.FieldGroup>
				<Field.Field>
					<Field.FieldLabel for="new-host">Hostname</Field.FieldLabel>
					<!-- svelte-ignore a11y_autofocus -->
					<Input id="new-host" bind:value={hostname} placeholder="www.esempio.it" autofocus required />
				</Field.Field>
				<Field.Field>
					<Field.FieldLabel>Tipo</Field.FieldLabel>
					<ToggleGroup.Root type="single" variant="outline" bind:value={() => kind, (v) => v && (kind = v as 'alias' | 'redirect')}>
						<ToggleGroup.Item value="redirect">Redirect</ToggleGroup.Item>
						<ToggleGroup.Item value="alias">Alias</ToggleGroup.Item>
					</ToggleGroup.Root>
					<p class="text-muted-foreground text-sm">
						{kind === 'redirect' ? 'Rimanda al dominio principale con un 301: di solito per www o domini vecchi.' : 'Stesso sito a questo indirizzo, senza rimandare.'}
					</p>
				</Field.Field>
				<Dialog.Footer>
					<Button variant="ghost" type="button" onclick={() => (adding = false)}>Annulla</Button>
					<Button variant="primary" type="submit">Aggiungi</Button>
				</Dialog.Footer>
			</Field.FieldGroup>
		</form>
	</Dialog.Content>
</Dialog.Root>
