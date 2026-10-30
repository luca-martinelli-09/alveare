<script lang="ts">
	// Log di un job in corso o finito, aggiornato ogni secondo.
	import { Badge } from '$lib/components/ui/badge';
	import { Spinner } from '$lib/components/ui/spinner';
	import { waitJob, errorMessage, type Job } from '$lib/api';

	let { id, ondone }: { id: number; ondone?: (job: Job) => void } = $props();
	let job = $state<Job | null>(null);
	let error = $state('');

	$effect(() => {
		const current = id;
		waitJob(current, (j) => (job = j))
			.then((j) => ondone?.(j))
			.catch((e) => (error = errorMessage(e)));
	});
</script>

<div class="flex flex-col gap-2">
	<div class="flex items-center gap-2">
		{#if !job || job.status === 'running'}
			<Spinner />
			<span class="text-sm">In corso…</span>
		{:else if job.status === 'done'}
			<Badge>Completato</Badge>
		{:else}
			<Badge variant="destructive">Fallito</Badge>
		{/if}
	</div>
	<pre class="bg-muted max-h-72 w-full min-w-0 overflow-auto rounded p-3 text-xs whitespace-pre-wrap [overflow-wrap:anywhere]">{(job?.log ?? []).join('\n') || error}</pre>
</div>
