<script lang="ts">
	// Attività recente: gli ultimi job (backup, ripristini, aggiornamenti, deploy) dei siti visibili.
	import { ago } from '$lib/health';
	import ArchiveIcon from '@lucide/svelte/icons/archive';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import RotateCcwIcon from '@lucide/svelte/icons/rotate-ccw';
	import CircleCheckIcon from '@lucide/svelte/icons/circle-check';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import GitBranchIcon from '@lucide/svelte/icons/git-branch';
	import CopyIcon from '@lucide/svelte/icons/copy';
	import TrashIcon from '@lucide/svelte/icons/trash-2';
	import TerminalIcon from '@lucide/svelte/icons/terminal';
	import ClockIcon from '@lucide/svelte/icons/clock';

	export type Activity = { id: number; kind: string; site_id: number | null; status: string; detail: string; started_at: string };
	let { items, siteName }: { items: Activity[]; siteName: (id: number | null) => string } = $props();

	const kinds: Record<string, { label: string; icon: typeof ArchiveIcon }> = {
		'site.backup': { label: 'backup', icon: ArchiveIcon },
		'site.restore': { label: 'ripristino', icon: RotateCcwIcon },
		'site.restore_test': { label: 'restore di prova', icon: CircleCheckIcon },
		'wordpress.update': { label: 'aggiornamento WordPress', icon: RefreshCwIcon },
		'wordpress.nightly': { label: 'aggiornamenti notturni', icon: RefreshCwIcon },
		'site.create': { label: 'creazione', icon: PlusIcon },
		create_site: { label: 'creazione', icon: PlusIcon },
		'site.import': { label: 'import', icon: PlusIcon },
		'site.clone': { label: 'clonazione', icon: CopyIcon },
		'site.export': { label: 'esportazione', icon: CopyIcon },
		'site.template': { label: 'salvato come template', icon: CopyIcon },
		'site.deploy': { label: 'deploy', icon: GitBranchIcon },
		'site.delete': { label: 'eliminazione', icon: TrashIcon },
		delete_site: { label: 'eliminazione', icon: TrashIcon },
		'cron.run': { label: 'cron', icon: ClockIcon },
		wp: { label: 'WP-CLI', icon: TerminalIcon }
	};
	const kindOf = (k: string) => kinds[k] ?? { label: k, icon: TerminalIcon };
</script>

<ul class="divide-y rounded-lg border">
	{#each items as a (a.id)}
		{@const k = kindOf(a.kind)}
		<li class="flex items-center gap-3 px-4 py-2.5">
			<span class="bg-muted text-muted-foreground flex size-7 shrink-0 items-center justify-center rounded-md" aria-hidden="true"><k.icon class="size-3.5" /></span>
			<p class="min-w-0 flex-1 truncate text-sm">
				{#if a.site_id}<a href={`/sites/${a.site_id}`} class="font-medium hover:underline">{siteName(a.site_id)}</a>{:else}<span class="font-medium">Pannello</span>{/if}
				<span class={a.status === 'failed' ? 'text-destructive' : 'text-muted-foreground'}>
					{k.label}{a.status === 'failed' ? ' fallito' : a.status === 'running' ? ' in corso' : ''}{a.detail ? ` · ${a.detail}` : ''}
				</span>
			</p>
			<time class="text-muted-foreground shrink-0 text-xs" datetime={a.started_at}>{ago(a.started_at)}</time>
		</li>
	{:else}
		<li class="text-muted-foreground px-4 py-6 text-center text-sm">Ancora nessuna operazione.</li>
	{/each}
</ul>
