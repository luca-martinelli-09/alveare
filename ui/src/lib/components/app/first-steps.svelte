<script lang="ts">
	// Primi passi dell'admin su un server nuovo: il primo sito e ciò che manca nel .env.
	import { Button } from '$lib/components/ui/button';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import { api } from '$lib/api';
	import CircleCheckIcon from '@lucide/svelte/icons/circle-check';
	import CircleIcon from '@lucide/svelte/icons/circle';
	import PlusIcon from '@lucide/svelte/icons/plus';

	let { oncreate }: { oncreate: () => void } = $props();

	type Setup = { backups: boolean; cloudflare: boolean; email: boolean };
	let setup = $state<Setup | null>(null);
	$effect(() => {
		api.get<{ setup: Setup }>('/api/server').then((s) => (setup = s.setup));
	});

	// Ciò che si configura dal .env: fatto o no, con le variabili da impostare.
	const env = $derived(
		setup
			? [
					{ done: setup.backups, label: 'Backup notturni', help: 'Su uno storage S3 come Cloudflare R2.', vars: 'RESTIC_REPOSITORY, AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY' },
					{ done: setup.cloudflare, label: 'Cloudflare via API', help: 'Gli hostname dei siti vanno da soli sul tunnel, con le regole della zona.', vars: 'CLOUDFLARE_<ETICHETTA>_API_TOKEN, _ACCOUNT_ID, _TUNNEL_ID' },
					{ done: setup.email, label: 'Email del pannello', help: 'Per invitare utenti e reimpostare le password via email.', vars: 'PANEL_SMTP_HOST, _USER, _PASSWORD, _FROM' }
				]
			: []
	);
</script>

{#snippet status(done: boolean)}
	{#if done}
		<CircleCheckIcon class="text-success mt-0.5 size-5 shrink-0" aria-label="Fatto" />
	{:else}
		<CircleIcon class="text-muted-foreground/60 mt-0.5 size-5 shrink-0" aria-label="Da fare" />
	{/if}
{/snippet}

<section class="flex flex-col gap-2" aria-labelledby="first-steps-title">
	<div class="flex flex-col gap-0.5">
		<h2 id="first-steps-title" class="text-sm font-medium">Primi passi</h2>
		<p class="text-muted-foreground text-xs">Il server è pronto. Questo elenco sparisce quando c'è il primo sito.</p>
	</div>
	<ol class="bg-card divide-y rounded-lg border">
		<li class="flex flex-wrap items-start gap-x-3 gap-y-3 px-4 py-3.5">
			{@render status(false)}
			<div class="min-w-0 flex-[1_1_16rem]">
				<p class="font-medium">Crea il primo sito</p>
				<p class="text-muted-foreground mt-0.5 text-sm">Da un template WordPress, PHP o Laravel, oppure importalo da un altro hosting.</p>
			</div>
			<Button size="sm" class="ml-8 sm:ml-0" onclick={oncreate}><PlusIcon data-icon="inline-start" /> Nuovo sito</Button>
		</li>
		<li class="flex flex-wrap items-start gap-x-3 gap-y-3 px-4 py-3.5">
			{@render status(false)}
			<div class="min-w-0 flex-[1_1_16rem]">
				<p class="font-medium">Scarica il kit di ripristino</p>
				<p class="text-muted-foreground mt-0.5 text-sm">Chiavi e password per ricostruire il server: conservalo fuori da qui.</p>
			</div>
			<Button size="sm" class="ml-8 sm:ml-0" href="/account?tab=recovery">Apri</Button>
		</li>
		{#if setup === null}
			<li class="px-4 py-3.5"><Skeleton class="h-10 w-full" /></li>
		{:else}
			{#each env as step (step.label)}
				<li class="flex items-start gap-3 px-4 py-3.5">
					{@render status(step.done)}
					<div class="min-w-0 flex-1">
						<p class="font-medium">{step.label}{#if step.done}<span class="text-muted-foreground ml-1.5 font-normal">· configurato</span>{/if}</p>
						<p class="text-muted-foreground mt-0.5 text-sm">{step.help}</p>
						{#if !step.done}
							<p class="text-muted-foreground mt-1.5 text-xs">Nel <code class="text-foreground">.env</code>: <code class="text-foreground break-words">{step.vars}</code>, poi riavvia il pannello.</p>
						{/if}
					</div>
				</li>
			{/each}
		{/if}
	</ol>
</section>
