<script lang="ts">
	// Il .env del sito, modificabile dagli admin. Database e Redis li usa anche il pannello: restano com'erano.
	import SettingsSection from '$lib/components/app/settings-section.svelte';
	import SettingRow from '$lib/components/app/setting-row.svelte';
	import CodeEditor from '$lib/components/app/code-editor.svelte';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as Alert from '$lib/components/ui/alert';
	import { Button } from '$lib/components/ui/button';
	import { Spinner } from '$lib/components/ui/spinner';
	import { api, errorMessage } from '$lib/api';
	import { toast } from 'svelte-sonner';
	import FileCogIcon from '@lucide/svelte/icons/file-cog';

	let { id }: { id: number } = $props();

	let open = $state(false);
	let content = $state('');
	let original = $state('');
	let locked = $state<string[]>([]);
	let error = $state('');
	let saving = $state(false);

	async function edit() {
		try {
			const r = await api.get<{ content: string; protected: string[] }>(`/api/sites/${id}/env`);
			[content, original, locked, error, open] = [r.content, r.content, r.protected, '', true];
		} catch (e) {
			toast.error(errorMessage(e));
		}
	}

	async function save() {
		saving = true;
		error = '';
		try {
			const r = await api.put<{ changed: string[] }>(`/api/sites/${id}/env`, { content });
			toast.success(r.changed.length ? `Salvato: ${r.changed.join(', ')}` : 'Nessuna modifica');
			open = false;
		} catch (e) {
			error = errorMessage(e);
		} finally {
			saving = false;
		}
	}
</script>

<SettingsSection title="Variabili d'ambiente">
	<SettingRow label=".env del sito" help="Letto da WordPress a ogni richiesta (Laravel: dopo php artisan config:clear). Database e Redis non si cambiano da qui.">
		<Button size="sm" onclick={edit}><FileCogIcon data-icon="inline-start" /> Modifica</Button>
	</SettingRow>
</SettingsSection>

<Dialog.Root bind:open>
	<Dialog.Content class="sm:max-w-4xl">
		<Dialog.Header>
			<Dialog.Title>.env del sito</Dialog.Title>
			<Dialog.Description>
				Una variabile per riga, NOME='valore'. Non si cambiano: <span class="font-mono">{locked.join(', ')}</span>.
			</Dialog.Description>
		</Dialog.Header>
		{#if error}<Alert.Root variant="destructive"><Alert.Description>{error}</Alert.Description></Alert.Root>{/if}
		{#if open}<CodeEditor bind:value={content} filename=".env" class="h-[55vh]" />{/if}
		<Dialog.Footer>
			<Button variant="ghost" onclick={() => (open = false)}>Annulla</Button>
			<Button variant="primary" onclick={save} disabled={saving || content === original}>
				{#if saving}<Spinner data-icon="inline-start" />{/if}
				Salva
			</Button>
		</Dialog.Footer>
	</Dialog.Content>
</Dialog.Root>
