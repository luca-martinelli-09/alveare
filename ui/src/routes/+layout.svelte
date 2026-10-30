<script lang="ts">
	import './layout.css';
	import favicon from '$lib/assets/favicon.svg';
	import type { LayoutProps } from './$types';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { Toaster } from '$lib/components/ui/sonner';
	import { Button } from '$lib/components/ui/button';
	import { Separator } from '$lib/components/ui/separator';
	import { Spinner } from '$lib/components/ui/spinner';
	import { api } from '$lib/api';
	import { session, loadSession } from '$lib/session.svelte';
	import LogOutIcon from '@lucide/svelte/icons/log-out';

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

	async function logout() {
		await api.post('/api/auth/logout').catch(() => {});
		session.user = null;
		session.stage = null;
		await goto('/login');
	}

	const links = $derived([
		{ href: '/', label: 'Siti' },
		...(session.user?.is_admin ? [{ href: '/users', label: 'Utenti' }, { href: '/server', label: 'Server' }] : []),
		{ href: '/account', label: 'Account' }
	]);
</script>

<svelte:head>
	<link rel="icon" href={favicon} />
	<title>Alveare</title>
</svelte:head>

<Toaster />

{#if isLogin}
	{@render children()}
{:else if session.stage === 'full'}
	<div class="flex min-h-svh flex-col">
		<header class="flex items-center gap-6 px-6 py-3">
			<a href="/" class="font-semibold">Alveare</a>
			<nav class="flex gap-1">
				{#each links as link (link.href)}
					<Button
						href={link.href}
						variant={page.url.pathname === link.href || (link.href === '/' && page.url.pathname.startsWith('/sites')) ? 'secondary' : 'ghost'}
						size="sm">{link.label}</Button
					>
				{/each}
			</nav>
			<div class="ml-auto flex items-center gap-3">
				<span class="text-muted-foreground text-sm">{session.user?.email}</span>
				<Button variant="ghost" size="sm" onclick={logout}>
					<LogOutIcon data-icon="inline-start" />
					Esci
				</Button>
			</div>
		</header>
		<Separator />
		<main class="mx-auto flex w-full max-w-6xl flex-1 flex-col gap-6 p-6">
			{@render children()}
		</main>
	</div>
{:else}
	<div class="flex min-h-svh items-center justify-center">
		<Spinner />
	</div>
{/if}
