<script lang="ts">
	// Utente e password in due campi di sola lettura, ognuno con il suo bottone per copiare.
	// `masked`: password nascosta finché non la si mostra (credenziali che restano visibili nel tempo).
	import * as InputGroup from '$lib/components/ui/input-group';
	import { toast } from 'svelte-sonner';
	import CopyIcon from '@lucide/svelte/icons/copy';
	import CheckIcon from '@lucide/svelte/icons/check';
	import EyeIcon from '@lucide/svelte/icons/eye';
	import EyeOffIcon from '@lucide/svelte/icons/eye-off';

	let { user, password, id, masked = false }: { user: string; password: string; id: string; masked?: boolean } = $props();
	let shown = $state(false);
	let copied = $state<string | null>(null);

	async function copy(key: string, value: string) {
		try {
			await navigator.clipboard.writeText(value);
			copied = key;
			setTimeout(() => copied === key && (copied = null), 1500);
		} catch {
			toast.error('Copia non riuscita: seleziona il testo e copialo a mano');
		}
	}

	const fields = $derived([
		{ key: 'user', label: 'Utente', value: user, hidden: false },
		{ key: 'password', label: 'Password', value: password, hidden: masked && !shown }
	]);
</script>

<div class="grid gap-3 sm:grid-cols-2">
	{#each fields as f (f.key)}
		<div class="flex min-w-0 flex-col gap-1.5">
			<label for={`${id}-${f.key}`} class="text-muted-foreground text-xs font-medium">{f.label}</label>
			<InputGroup.Root>
				<InputGroup.Input id={`${id}-${f.key}`} readonly type={f.hidden ? 'password' : 'text'} value={f.value} class="font-mono" onfocus={(e) => e.currentTarget.select()} />
				<InputGroup.Addon align="inline-end">
					{#if f.key === 'password' && masked}
						<InputGroup.Button size="icon-xs" aria-label={shown ? 'Nascondi password' : 'Mostra password'} title={shown ? 'Nascondi' : 'Mostra'} onclick={() => (shown = !shown)}>
							{#if shown}<EyeOffIcon />{:else}<EyeIcon />{/if}
						</InputGroup.Button>
					{/if}
					<InputGroup.Button size="icon-xs" aria-label={`Copia ${f.label.toLowerCase()}`} title="Copia" onclick={() => copy(f.key, f.value)}>
						{#if copied === f.key}<CheckIcon class="text-success" />{:else}<CopyIcon />{/if}
					</InputGroup.Button>
				</InputGroup.Addon>
			</InputGroup.Root>
		</div>
	{/each}
</div>
