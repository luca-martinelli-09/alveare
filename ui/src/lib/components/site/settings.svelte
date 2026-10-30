<script lang="ts">
	import { askConfirm } from '$lib/ask.svelte';
	// Impostazioni del sito: PHP dal form (gestori), override grezzi (admin), IP bloccati.
	import SettingsSection from '$lib/components/app/settings-section.svelte';
	import SettingRow from '$lib/components/app/setting-row.svelte';
	import * as Alert from '$lib/components/ui/alert';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import * as Select from '$lib/components/ui/select';
	import OverrideEditor from '$lib/components/app/override-editor.svelte';
	import { api, ApiError, errorMessage } from '$lib/api';
	import { session } from '$lib/session.svelte';
	import { toast } from 'svelte-sonner';
	import type { SiteDetail } from './types';
	import EmailCard from './email-card.svelte';
	import EnvEditor from './env-editor.svelte';

	let { detail, onchange }: { detail: SiteDetail; onchange: () => void } = $props();
	type Block = { id: number; site_id: number | null; cidr: string; expires_at: number | null; created_at: number };
	const id = $derived(detail.site.id);
	let php = $state({ memory_limit_mb: 0, upload_max_mb: 0, max_execution_time_s: 0, max_children: 0 });
	$effect(() => {
		php = { ...detail.site.settings };
	});
	let warning = $state('');
	let confirmText = $state('');
	let blocks = $state<Block[]>([]);

	async function loadBlocks() {
		blocks = await api.get<Block[]>(`/api/sites/${id}/blocks`);
	}

	$effect(() => {
		loadBlocks();
	});

	async function savePhp(confirm_budget = false) {
		try {
			const r = await api.patch<{ warning: string | null }>(`/api/sites/${id}`, { php, confirm_budget });
			warning = r.warning ?? '';
			confirmText = '';
			toast.success('Impostazioni PHP applicate');
			onchange();
		} catch (e) {
			if (e instanceof ApiError && e.status === 409) confirmText = e.message;
			else toast.error(errorMessage(e));
		}
	}

	async function setRuntime(v: string) {
		if (v === detail.site.php_version) return;
		if (!(await askConfirm({ title: `Spostare il sito su PHP ${v}?`, description: 'Per qualche secondo può rispondere 502.', confirmLabel: 'Sposta' }))) return;
		try {
			await api.patch(`/api/sites/${id}`, { php_version: v });
			toast.success(`Sito su PHP ${v}`);
			onchange();
		} catch (e) {
			toast.error(errorMessage(e));
		}
	}

	async function unblock(b: Block) {
		try {
			await api.del(`/api/blocks/${b.id}`);
			await loadBlocks();
		} catch (e) {
			toast.error(errorMessage(e));
		}
	}

	const fields = [
		{ key: 'memory_limit_mb', label: 'memory_limit', help: 'Memoria massima per richiesta PHP.', unit: 'MB', min: 32, max: 4096 },
		{ key: 'upload_max_mb', label: 'Upload massimo', help: 'Dimensione massima di un file caricato.', unit: 'MB', min: 1, max: 4096 },
		{ key: 'max_execution_time_s', label: 'Tempo massimo', help: 'Oltre questo una richiesta viene interrotta.', unit: 's', min: 5, max: 3600 },
		{ key: 'max_children', label: 'Processi massimi', help: 'Richieste servite in parallelo dal pool.', unit: '', min: 1, max: 100 }
	] as const;
	const changed = $derived(fields.filter((f) => Number(php[f.key]) !== detail.site.settings[f.key]));
	const when = (t: number | null) => (t ? new Date(t * 1000).toLocaleString('it-IT', { dateStyle: 'short', timeStyle: 'short' }) : 'mai');
</script>

<div class="flex flex-col gap-6 pt-6">
	<SettingsSection title="PHP" description="Valori del pool del sito. Massimo teorico di memoria: processi × memory_limit; oltre la RAM del server compare un avviso.">
		{#if warning || confirmText}
			<div class="px-4 py-3">
				{#if warning}<Alert.Root><Alert.Description>{warning}</Alert.Description></Alert.Root>{/if}
				{#if confirmText}
					<Alert.Root variant="destructive">
						<Alert.Title>Serve una conferma</Alert.Title>
						<Alert.Description>{confirmText}</Alert.Description>
					</Alert.Root>
				{/if}
			</div>
		{/if}
		{#if detail.php_runtimes.length > 1}
			<SettingRow label="Versione di PHP" help={'Ogni versione è un container a parte; "canary" è l\'immagine nuova in prova.'}>
				<Select.Root type="single" value={detail.site.php_version} onValueChange={setRuntime}>
					<Select.Trigger class="w-36" aria-label="Versione di PHP">PHP {detail.site.php_version}</Select.Trigger>
					<Select.Content>
						<Select.Group>
							{#each detail.php_runtimes as rt (rt)}
								<Select.Item value={rt}>PHP {rt}</Select.Item>
							{/each}
						</Select.Group>
					</Select.Content>
				</Select.Root>
			</SettingRow>
		{/if}
		{#each fields as f (f.key)}
			<SettingRow label={f.label} labelFor={f.key} help={f.help}>
				<Input
					id={f.key}
					type="number"
					min={f.min}
					max={f.max}
					class={['w-24 text-right tabular-nums', Number(php[f.key]) !== detail.site.settings[f.key] && 'border-primary bg-accent']}
					bind:value={php[f.key]}
				/>
				<span class="text-muted-foreground w-6 text-sm">{f.unit}</span>
			</SettingRow>
		{/each}
		{#if changed.length}
			<div class="bg-muted/50 flex flex-wrap items-center justify-end gap-2 rounded-b-lg px-4 py-2.5">
				<span class="text-muted-foreground mr-auto text-sm">
					{changed.length === 1 ? '1 modifica' : `${changed.length} modifiche`}: {changed.map((f) => `${f.label} ${detail.site.settings[f.key]} → ${php[f.key]}${f.unit ? ' ' + f.unit : ''}`).join(', ')}
				</span>
				<Button variant="ghost" size="sm" onclick={() => ((php = { ...detail.site.settings }), (confirmText = ''))}>Annulla</Button>
				{#if confirmText}<Button variant="destructive" size="sm" onclick={() => savePhp(true)}>Salva comunque</Button>
				{:else}<Button variant="primary" size="sm" onclick={() => savePhp()}>Salva modifiche</Button>{/if}
			</div>
		{/if}
	</SettingsSection>

	{#if session.user?.is_admin}
		<SettingsSection title="Configurazione avanzata">
			<OverrideEditor
				base={`/api/sites/${id}/config/php`}
				title="Direttive PHP extra"
				description="php_value e php_admin_value aggiunte al pool, provate con php-fpm -t; open_basedir, disable_functions e simili sono rifiutati."
				filename="pool.ini"
			/>
			<OverrideEditor
				base={`/api/sites/${id}/config/caddy`}
				title="Caddy del sito"
				description="Snippet nel blocco del sito (header, redirect, cache, blocchi di path), validato con /adapt."
				filename="Caddyfile"
			/>
		</SettingsSection>
	{/if}

	<EmailCard id={detail.site.id} />
	{#if session.user?.is_admin}<EnvEditor id={detail.site.id} />{/if}

	<SettingsSection title="IP bloccati">
		{#each blocks as b (b.id)}
			<SettingRow help={`${b.site_id === null ? 'Tutti i siti' : 'Questo sito'} · scade ${when(b.expires_at)}`}>
				{#snippet label()}<code class="font-medium">{b.cidr}</code>{/snippet}
				{#if b.site_id !== null || session.user?.is_admin}
					<Button size="sm" onclick={() => unblock(b)}>Sblocca</Button>
				{/if}
			</SettingRow>
		{:else}
			<SettingRow label="Nessun IP bloccato" help="Si blocca dalla scheda Log, sulla riga della richiesta. Per fermare qualcuno prima del tunnel usa Cloudflare." />
		{/each}
	</SettingsSection>
</div>
