<script lang="ts">
	// Pannello degli upload in basso a destra, come Google Drive: riepilogo e stato di ogni file.
	import { Button } from '$lib/components/ui/button';
	import { Progress } from '$lib/components/ui/progress';
	import { Spinner } from '$lib/components/ui/spinner';
	import { uploads, clearUploads } from '$lib/uploads.svelte';
	import { summary } from '$lib/upload-queue';
	import { formatBytes } from '$lib/api';
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import ChevronUpIcon from '@lucide/svelte/icons/chevron-up';
	import XIcon from '@lucide/svelte/icons/x';
	import CircleCheckIcon from '@lucide/svelte/icons/circle-check';
	import CircleAlertIcon from '@lucide/svelte/icons/circle-alert';
	import ClockIcon from '@lucide/svelte/icons/clock';
	import CircleMinusIcon from '@lucide/svelte/icons/circle-minus';
	import CircleHelpIcon from '@lucide/svelte/icons/circle-question-mark';
	import * as Dialog from '$lib/components/ui/dialog';
	import { Checkbox } from '$lib/components/ui/checkbox';
	import { Label } from '$lib/components/ui/label';

	// Risposta alla domanda "c'è già": con la spunta vale per il resto del caricamento.
	let all = $state(false);
	function answer(replace: boolean) {
		uploads.conflict?.resolve({ replace, all });
		all = false;
	}

	const s = $derived(summary(uploads.items));
	const title = $derived(
		!s.finished
			? `Caricamento: ${s.done} di ${s.total}`
			: [`${s.done} ${s.done === 1 ? 'caricato' : 'caricati'}`, s.skipped && `${s.skipped} ${s.skipped === 1 ? 'saltato' : 'saltati'}`, s.failed && `${s.failed} non riusciti`]
					.filter(Boolean)
					.join(', ')
	);
</script>

{#if uploads.items.length}
	<section
		class="bg-popover text-popover-foreground fixed right-4 bottom-4 z-50 flex w-[min(24rem,calc(100vw-2rem))] flex-col overflow-hidden rounded-lg border shadow-lg"
		aria-label="Upload"
		aria-live="polite"
	>
		<header class="flex items-center gap-2 border-b py-2 pr-2 pl-4">
			<div class="min-w-0 flex-1">
				<p class="truncate text-sm font-medium">{title}</p>
				{#if !s.finished}
					<p class="text-muted-foreground text-xs">{s.percent}% · {s.waiting} in attesa{s.failed ? ` · ${s.failed} non riusciti` : ''}</p>
				{/if}
			</div>
			<Button variant="ghost" size="icon-sm" aria-label={uploads.minimized ? 'Espandi' : 'Riduci'} onclick={() => (uploads.minimized = !uploads.minimized)}>
				{#if uploads.minimized}<ChevronUpIcon />{:else}<ChevronDownIcon />{/if}
			</Button>
			{#if s.finished}<Button variant="ghost" size="icon-sm" aria-label="Chiudi" onclick={clearUploads}><XIcon /></Button>{/if}
		</header>
		{#if !s.finished}<Progress value={s.percent} class="h-0.5 rounded-none" />{/if}
		{#if !uploads.minimized}
			<ul class="max-h-72 divide-y overflow-y-auto">
				{#each uploads.items as i (i.id)}
					<li class="flex items-center gap-3 px-4 py-2">
						<span class="flex size-4 shrink-0 items-center justify-center [&_svg]:size-4">
							{#if i.status === 'done'}<CircleCheckIcon class="text-success" />
							{:else if i.status === 'error'}<CircleAlertIcon class="text-destructive" />
							{:else if i.status === 'skipped'}<CircleMinusIcon class="text-muted-foreground" />
							{:else if i.status === 'conflict'}<CircleHelpIcon class="text-warning" />
							{:else if i.status === 'uploading'}<Spinner />
							{:else}<ClockIcon class="text-muted-foreground" />{/if}
						</span>
						<div class="min-w-0 flex-1">
							<p class="truncate text-sm" title={i.name}>{i.name}</p>
							{#if i.status === 'uploading'}
								<Progress value={i.progress * 100} class="mt-1 h-1" />
							{:else if i.status === 'error'}
								<p class="text-destructive truncate text-xs" title={i.error ?? undefined}>{i.error}</p>
							{:else}
								<p class="text-muted-foreground text-xs">{formatBytes(i.size)}{{ waiting: ' · in attesa', conflict: ' · c\'è già: sostituire?', skipped: ' · saltato, c\'era già' }[i.status as string] ?? ''}</p>
							{/if}
						</div>
					</li>
				{/each}
			</ul>
		{/if}
	</section>
{/if}

<Dialog.Root open={uploads.conflict !== null} onOpenChange={(o) => !o && answer(false)}>
	<Dialog.Content class="sm:max-w-md">
		<Dialog.Header>
			<Dialog.Title>Il file esiste già</Dialog.Title>
			<Dialog.Description>
				<span class="text-foreground font-mono break-all">{uploads.conflict?.name}</span> c'è già in questa cartella. Lo sostituisci con quello che stai caricando?
			</Dialog.Description>
		</Dialog.Header>
		{#if (uploads.conflict?.others ?? 0) > 0}
			<div class="flex items-center gap-2">
				<Checkbox id="conflict-all" bind:checked={all} />
				<Label for="conflict-all" class="font-normal">Applica a tutti gli altri file di questo caricamento che esistono già</Label>
			</div>
		{/if}
		<Dialog.Footer>
			<Button variant="ghost" onclick={() => answer(false)}>Salta</Button>
			<Button variant="primary" onclick={() => answer(true)}>Sostituisci</Button>
		</Dialog.Footer>
	</Dialog.Content>
</Dialog.Root>
