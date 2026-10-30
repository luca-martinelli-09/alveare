<script lang="ts">
	import './layout.css';
	import favicon from '$lib/assets/favicon.svg';
	import type { LayoutProps } from './$types';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { ModeWatcher } from 'mode-watcher';
	import { Toaster } from '$lib/components/ui/sonner';
	import * as Sidebar from '$lib/components/ui/sidebar';
	import { Spinner } from '$lib/components/ui/spinner';
	import AppSidebar from '$lib/components/app/app-sidebar.svelte';
	import CommandPalette from '$lib/components/app/command-palette.svelte';
	import AskDialog from '$lib/components/app/ask-dialog.svelte';
	import { palette } from '$lib/palette.svelte';
	import SearchIcon from '@lucide/svelte/icons/search';
	import Logo from '$lib/components/app/logo.svelte';
	import { api } from '$lib/api';
	import { session, loadSession } from '$lib/session.svelte';
	import { loadSites } from '$lib/sites.svelte';

	let { children }: LayoutProps = $props();

	// Pagine raggiungibili senza sessione completa: login, invito, reset della password.
	const isLogin = $derived(['/login', '/invito', '/reset'].some((p) => page.url.pathname.startsWith(p)));

	$effect(() => {
		if (!session.loaded) {
			loadSession();
		} else if (!isLogin && session.stage !== 'full') {
			goto('/login');
		}
	});

	// Stato dei siti nella barra laterale: al login e ogni minuto.
	$effect(() => {
		if (session.stage !== 'full') return;
		loadSites();
		const timer = setInterval(loadSites, 60_000);
		return () => clearInterval(timer);
	});

	function shortcut(e: KeyboardEvent) {
		if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k' && session.stage === 'full') {
			e.preventDefault();
			palette.open = !palette.open;
		}
	}

	async function logout() {
		await api.post('/api/auth/logout').catch(() => {});
		session.user = null;
		session.stage = null;
		await goto('/login');
	}
</script>

<svelte:head>
	<link rel="icon" href={favicon} type="image/svg+xml" />
	<link rel="apple-touch-icon" href="/apple-touch-icon.png" />
	<title>Alveare</title>
</svelte:head>

<svelte:window onkeydown={shortcut} />
<ModeWatcher />
<Toaster />
<AskDialog />

{#if isLogin}
	{@render children()}
{:else if session.stage === 'full'}
	<Sidebar.Provider>
		<AppSidebar onlogout={logout} />
		<CommandPalette />
		<Sidebar.Inset class="bg-honey-wash">
			<header class="bg-background/90 sticky top-0 z-10 flex h-12 items-center gap-2 border-b px-3 backdrop-blur md:hidden">
				<Sidebar.Trigger />
				<a href="/" class="flex flex-1 items-center gap-2">
					<Logo class="size-6" />
					<span class="font-semibold tracking-tight">Alveare</span>
				</a>
				<button type="button" class="hover:bg-muted flex size-9 items-center justify-center rounded-md" aria-label="Cerca siti e azioni" onclick={() => (palette.open = true)}>
					<SearchIcon class="size-4" />
				</button>
			</header>
			<main class="mx-auto flex w-full max-w-[88rem] flex-1 flex-col gap-6 px-4 py-6 md:px-8 md:py-8">
				{@render children()}
			</main>
		</Sidebar.Inset>
	</Sidebar.Provider>
{:else}
	<div class="flex min-h-svh items-center justify-center">
		<Spinner />
	</div>
{/if}
