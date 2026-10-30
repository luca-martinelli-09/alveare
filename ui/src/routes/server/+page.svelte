<script lang="ts">
	import PageHeader from '$lib/components/app/page-header.svelte';
	// Pagina del server (admin): budget di memoria dei pool, override globali, IP bloccati.
	import SettingsSection from '$lib/components/app/settings-section.svelte';
	import SettingRow from '$lib/components/app/setting-row.svelte';
	import * as Table from '$lib/components/ui/table';
	import * as Alert from '$lib/components/ui/alert';
	import { Button } from '$lib/components/ui/button';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import OverrideEditor from '$lib/components/app/override-editor.svelte';
	import CloudflareCard from '$lib/components/app/cloudflare-card.svelte';
	import { api, errorMessage } from '$lib/api';
	import { toast } from 'svelte-sonner';

	type Budget = { pools_mb: number; reserved_mb: number; total_mb: number; ram_mb: number; percent: number };
	type SiteBudget = { id: number; slug: string; domain: string | null; status: string; pool_mb: number; settings: { memory_limit_mb: number; max_children: number } };
	type Block = { id: number; site_id: number | null; cidr: string; expires_at: number | null };
	let server = $state<{ budget: Budget; sites: SiteBudget[]; version: string; build: string | null } | null>(null);
	let blocks = $state<Block[]>([]);
	let perSite = $state(false);

	async function load() {
		[server, blocks] = await Promise.all([api.get<typeof server>('/api/server'), api.get<Block[]>('/api/blocks')]);
	}

	$effect(() => {
		load();
	});

	async function unblock(b: Block) {
		try {
			await api.del(`/api/blocks/${b.id}`);
			await load();
		} catch (e) {
			toast.error(errorMessage(e));
		}
	}

	const gb = (mb: number) => `${(mb / 1024).toFixed(1)} GB`;
	const siteName = (id: number | null) => (id === null ? 'tutti i siti' : (server?.sites.find((s) => s.id === id)?.domain ?? `sito ${id}`));
</script>

<PageHeader title="Server" description={server ? `Alveare ${server.version}${server.build && server.build !== `v${server.version}` ? ` (${server.build})` : ''} · ${server.sites.length} ${server.sites.length === 1 ? 'sito' : 'siti'} · ${gb(server.budget.ram_mb)} di RAM` : undefined} />

{#if !server}
	<Skeleton class="h-40 w-full" />
{:else}
	{@const b = server.budget}
	<section class="flex flex-col gap-2">
		<h2 class="text-sm font-medium">Memoria</h2>
		<div class="bg-card flex flex-col gap-2 rounded-lg border px-4 py-3.5">
			<div class="flex flex-wrap items-baseline justify-between gap-2">
				<p>
					<span class={['font-medium', b.percent > 100 && 'text-destructive-text']}>{gb(b.total_mb)}</span>
					<span class="text-muted-foreground">di {gb(b.ram_mb)} nel caso peggiore, con tutti i pool al massimo ({b.percent}%)</span>
				</p>
				<button type="button" class="text-muted-foreground hover:text-foreground text-sm underline underline-offset-2" onclick={() => (perSite = !perSite)}>
					{perSite ? 'Nascondi il dettaglio' : 'Dettaglio per sito'}
				</button>
			</div>
			<div class="bg-muted flex h-2 overflow-hidden rounded-full" aria-hidden="true">
				<div class={b.percent > 100 ? 'bg-destructive' : 'bg-honey'} style:width={`${Math.min(100, (b.pools_mb / b.ram_mb) * 100)}%`}></div>
				<div class="bg-muted-foreground/30" style:width={`${Math.min(100, (b.reserved_mb / b.ram_mb) * 100)}%`}></div>
			</div>
			<p class="text-muted-foreground text-xs">
				Pool PHP {gb(b.pools_mb)} · servizi (MariaDB, Redis, Caddy, pannello) {gb(b.reserved_mb)}. Con pm = ondemand i pool raramente sono tutti pieni.
			</p>
			{#if b.percent > 100}
				<Alert.Root variant="destructive"><Alert.Description>Il totale supera la RAM: abbassa processi o memory_limit dei siti più grandi.</Alert.Description></Alert.Root>
			{/if}
			{#if perSite}
				<div class="mt-1 rounded-md border">
					<Table.Root>
						<Table.Header>
							<Table.Row>
								<Table.Head class="pl-3">Sito</Table.Head>
								<Table.Head class="text-right">Processi</Table.Head>
								<Table.Head class="text-right">memory_limit</Table.Head>
								<Table.Head class="pr-3 text-right">Massimo</Table.Head>
							</Table.Row>
						</Table.Header>
						<Table.Body>
							{#each server.sites as s (s.id)}
								<Table.Row>
									<Table.Cell class="pl-3"><a class="underline-offset-4 hover:underline" href={`/sites/${s.id}?tab=settings`}>{s.domain ?? s.slug}</a></Table.Cell>
									<Table.Cell class="text-right">{s.settings.max_children}</Table.Cell>
									<Table.Cell class="text-right">{s.settings.memory_limit_mb} MB</Table.Cell>
									<Table.Cell class="pr-3 text-right">{gb(s.pool_mb)}</Table.Cell>
								</Table.Row>
							{/each}
						</Table.Body>
					</Table.Root>
				</div>
			{/if}
		</div>
	</section>

	<SettingsSection title="Configurazione globale">
		<OverrideEditor
			base="/api/config/php"
			title="PHP globale"
			description="conf.d/zz-custom.ini, letto da tutti i pool dopo le impostazioni dell'immagine; i valori che proteggono l'isolamento sono rifiutati."
			filename="zz-custom.ini"
		/>
		<OverrideEditor base="/api/config/caddy" title="Caddy globale" description="Snippet riusabili dai siti con import." filename="Caddyfile" />
	</SettingsSection>

	<CloudflareCard />

	<SettingsSection title="IP bloccati">
		{#each blocks as bl (bl.id)}
			<SettingRow help={`${siteName(bl.site_id)} · scade ${bl.expires_at ? new Date(bl.expires_at * 1000).toLocaleString('it-IT') : 'mai'}`}>
				{#snippet label()}<code class="font-medium">{bl.cidr}</code>{/snippet}
				<Button size="sm" onclick={() => unblock(bl)}>Sblocca</Button>
			</SettingRow>
		{:else}
			<SettingRow label="Nessun IP bloccato" help="Si blocca dalla scheda Log di un sito, sulla riga della richiesta." />
		{/each}
	</SettingsSection>
{/if}
