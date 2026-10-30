<script lang="ts">
	import PageHeader from '$lib/components/app/page-header.svelte';
	// Pagina del server (admin): budget di memoria dei pool, override globali, IP bloccati.
	import * as Card from '$lib/components/ui/card';
	import * as Table from '$lib/components/ui/table';
	import * as Alert from '$lib/components/ui/alert';
	import { Button } from '$lib/components/ui/button';
	import { Progress } from '$lib/components/ui/progress';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import OverrideEditor from '$lib/components/app/override-editor.svelte';
	import CloudflareCard from '$lib/components/app/cloudflare-card.svelte';
	import { api, errorMessage } from '$lib/api';
	import { toast } from 'svelte-sonner';

	type Budget = { pools_mb: number; reserved_mb: number; total_mb: number; ram_mb: number; percent: number };
	type SiteBudget = { id: number; slug: string; domain: string | null; status: string; pool_mb: number; settings: { memory_limit_mb: number; max_children: number } };
	type Block = { id: number; site_id: number | null; cidr: string; expires_at: number | null };
	let server = $state<{ budget: Budget; sites: SiteBudget[]; version: string } | null>(null);
	let blocks = $state<Block[]>([]);

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

<PageHeader title="Server" description={server ? `Alveare ${server.version} · memoria, PHP globale, Caddy, Cloudflare e import` : undefined} />

{#if !server}
	<Skeleton class="h-40 w-full" />
{:else}
	<Card.Root>
		<Card.Header>
			<Card.Title>Budget di memoria</Card.Title>
			<Card.Description>
				Massimo teorico dei pool (processi × memory_limit) più {gb(server.budget.reserved_mb)} per MariaDB, Redis, Caddy e pannello. Con pm = ondemand i pool
				raramente sono tutti al massimo insieme.
			</Card.Description>
		</Card.Header>
		<Card.Content class="flex flex-col gap-3">
			<div class="flex items-baseline justify-between text-sm">
				<span>{gb(server.budget.total_mb)} su {gb(server.budget.ram_mb)} di RAM</span>
				<span class={server.budget.percent > 100 ? 'text-destructive font-medium' : 'text-muted-foreground'}>{server.budget.percent}%</span>
			</div>
			<Progress value={Math.min(server.budget.percent, 100)} />
			{#if server.budget.percent > 100}
				<Alert.Root variant="destructive"><Alert.Description>Il totale supera la RAM: abbassa processi o memory_limit dei siti più grandi.</Alert.Description></Alert.Root>
			{/if}
			<Table.Root>
				<Table.Header>
					<Table.Row>
						<Table.Head>Sito</Table.Head>
						<Table.Head class="text-right">Processi</Table.Head>
						<Table.Head class="text-right">memory_limit</Table.Head>
						<Table.Head class="text-right">Massimo</Table.Head>
					</Table.Row>
				</Table.Header>
				<Table.Body>
					{#each server.sites as s (s.id)}
						<Table.Row>
							<Table.Cell><a class="underline-offset-4 hover:underline" href={`/sites/${s.id}`}>{s.domain ?? s.slug}</a></Table.Cell>
							<Table.Cell class="text-right">{s.settings.max_children}</Table.Cell>
							<Table.Cell class="text-right">{s.settings.memory_limit_mb} MB</Table.Cell>
							<Table.Cell class="text-right">{gb(s.pool_mb)}</Table.Cell>
						</Table.Row>
					{/each}
				</Table.Body>
			</Table.Root>
		</Card.Content>
	</Card.Root>

	<OverrideEditor
		base="/api/config/php"
		title="PHP globale"
		description="conf.d/zz-custom.ini, letto da tutti i pool dopo le impostazioni dell'immagine. Solo chiave = valore; i valori che proteggono l'isolamento sono rifiutati."
		filename="zz-custom.ini"
	/>
	<OverrideEditor
		base="/api/config/caddy"
		title="Caddy globale"
		description="Snippet riusabili dai siti con import, per esempio (sicurezza) {'{'} header X-Frame-Options DENY {'}'}."
		filename="Caddyfile"
	/>

	<CloudflareCard />

	<Card.Root>
		<Card.Header>
			<Card.Title>IP bloccati</Card.Title>
			<Card.Description>Tutti i blocchi attivi, con l'ambito e la scadenza.</Card.Description>
		</Card.Header>
		<Card.Content>
			<Table.Root>
				<Table.Body>
					{#each blocks as b (b.id)}
						<Table.Row>
							<Table.Cell><code>{b.cidr}</code></Table.Cell>
							<Table.Cell>{siteName(b.site_id)}</Table.Cell>
							<Table.Cell class="text-muted-foreground">scade: {b.expires_at ? new Date(b.expires_at * 1000).toLocaleString('it-IT') : 'mai'}</Table.Cell>
							<Table.Cell class="text-right"><Button variant="outline" size="sm" onclick={() => unblock(b)}>Sblocca</Button></Table.Cell>
						</Table.Row>
					{:else}
						<Table.Row><Table.Cell class="text-muted-foreground">Nessun IP bloccato.</Table.Cell></Table.Row>
					{/each}
				</Table.Body>
			</Table.Root>
		</Card.Content>
	</Card.Root>
{/if}
