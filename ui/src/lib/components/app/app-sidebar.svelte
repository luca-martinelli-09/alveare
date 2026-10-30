<script lang="ts">
	import { page } from '$app/state';
	import { afterNavigate } from '$app/navigation';
	import * as Sidebar from '$lib/components/ui/sidebar';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import Logo from './logo.svelte';
	import StatusDot from './status-dot.svelte';
	import { session } from '$lib/session.svelte';
	import { sites } from '$lib/sites.svelte';
	import { health, byHealth } from '$lib/health';
	import { setMode, userPrefersMode } from 'mode-watcher';
	import { palette } from '$lib/palette.svelte';
	import SearchIcon from '@lucide/svelte/icons/search';
	import LayoutGridIcon from '@lucide/svelte/icons/layout-grid';
	import LayoutTemplateIcon from '@lucide/svelte/icons/layout-template';
	import UsersIcon from '@lucide/svelte/icons/users';
	import ServerIcon from '@lucide/svelte/icons/server';
	import ChevronsUpDownIcon from '@lucide/svelte/icons/chevrons-up-down';
	import UserIcon from '@lucide/svelte/icons/user';
	import LogOutIcon from '@lucide/svelte/icons/log-out';
	import SunMoonIcon from '@lucide/svelte/icons/sun-moon';

	let { onlogout }: { onlogout: () => void } = $props();
	const sidebar = Sidebar.useSidebar();
	let filter = $state('');

	// Su telefono il foglio si chiude appena si cambia pagina.
	afterNavigate(() => sidebar.setOpenMobile(false));

	const path = $derived(page.url.pathname);
	const list = $derived(
		(sites.list ?? []).filter((s) => (s.domain ?? s.slug).includes(filter.trim().toLowerCase())).toSorted(byHealth)
	);
	const admin = $derived([
		{ href: '/templates', label: 'Template', icon: LayoutTemplateIcon },
		{ href: '/users', label: 'Utenti', icon: UsersIcon },
		{ href: '/server', label: 'Server', icon: ServerIcon }
	]);
	const initials = $derived((session.user?.name ?? '?').split(/\s+/).map((w) => w[0]).slice(0, 2).join('').toUpperCase());
	const themes = { system: 'Come il sistema', light: 'Chiaro', dark: 'Scuro' } as const;
</script>

<Sidebar.Root>
	<Sidebar.Header>
		<a href="/" class="flex items-center gap-2 px-2 py-1.5">
			<Logo />
			<span class="text-[15px] font-semibold tracking-tight">Alveare</span>
		</a>
		<button
			type="button"
			class="border-sidebar-border bg-background text-muted-foreground hover:text-foreground flex h-9 items-center gap-2 rounded-md border px-2.5 text-sm"
			onclick={() => (palette.open = true)}
		>
			<SearchIcon class="size-4" />
			<span class="flex-1 text-left">Cerca siti e azioni</span>
			<kbd class="border-sidebar-border rounded border px-1.5 font-mono text-[11px]">⌘K</kbd>
		</button>
	</Sidebar.Header>
	<Sidebar.Content>
		<Sidebar.Group>
			<Sidebar.Menu>
				<Sidebar.MenuItem>
					<Sidebar.MenuButton isActive={path === '/'}>
						{#snippet child({ props })}
							<a href="/" {...props}><LayoutGridIcon /><span>Panoramica</span></a>
						{/snippet}
					</Sidebar.MenuButton>
				</Sidebar.MenuItem>
			</Sidebar.Menu>
		</Sidebar.Group>
		<Sidebar.Group>
			<Sidebar.GroupLabel>Siti</Sidebar.GroupLabel>
			{#if (sites.list?.length ?? 0) > 8}
				<Sidebar.Input placeholder="Cerca un sito" aria-label="Cerca un sito" bind:value={filter} class="mb-1" />
			{/if}
			<Sidebar.Menu>
				{#each list as s (s.id)}
					{@const h = health(s)}
					<Sidebar.MenuItem>
						<Sidebar.MenuButton isActive={path === `/sites/${s.id}`} size="sm">
							{#snippet child({ props })}
								<a href={`/sites/${s.id}`} {...props}>
									<StatusDot level={h.level} label={h.label} class="mx-1" />
									<span class="truncate">{s.domain ?? s.slug}</span>
								</a>
							{/snippet}
						</Sidebar.MenuButton>
					</Sidebar.MenuItem>
				{:else}
					{#if sites.list}<p class="text-muted-foreground px-2 py-1 text-xs">Nessun sito.</p>{/if}
				{/each}
			</Sidebar.Menu>
		</Sidebar.Group>
		{#if session.user?.is_admin}
			<Sidebar.Group>
				<Sidebar.GroupLabel>Amministrazione</Sidebar.GroupLabel>
				<Sidebar.Menu>
					{#each admin as item (item.href)}
						<Sidebar.MenuItem>
							<Sidebar.MenuButton isActive={path.startsWith(item.href)}>
								{#snippet child({ props })}
									<a href={item.href} {...props}><item.icon /><span>{item.label}</span></a>
								{/snippet}
							</Sidebar.MenuButton>
						</Sidebar.MenuItem>
					{/each}
				</Sidebar.Menu>
			</Sidebar.Group>
		{/if}
	</Sidebar.Content>
	<Sidebar.Footer>
		<Sidebar.Menu>
			<Sidebar.MenuItem>
				<DropdownMenu.Root>
					<DropdownMenu.Trigger>
						{#snippet child({ props })}
							<Sidebar.MenuButton size="lg" {...props}>
								<span class="bg-muted text-foreground flex size-8 items-center justify-center rounded-md text-xs font-medium">{initials}</span>
								<span class="flex min-w-0 flex-col text-left leading-tight">
									<span class="truncate text-sm font-medium">{session.user?.name}</span>
									<span class="text-muted-foreground truncate text-xs">{session.user?.email}</span>
								</span>
								<ChevronsUpDownIcon class="ml-auto" />
							</Sidebar.MenuButton>
						{/snippet}
					</DropdownMenu.Trigger>
					<DropdownMenu.Content side="top" align="start" class="w-(--bits-dropdown-menu-anchor-width) min-w-56">
						<DropdownMenu.Group>
							<DropdownMenu.Item>
								{#snippet child({ props })}
									<a href="/account" {...props}><UserIcon /> Account</a>
								{/snippet}
							</DropdownMenu.Item>
						</DropdownMenu.Group>
						<DropdownMenu.Separator />
						<DropdownMenu.Group>
							<DropdownMenu.Label class="flex items-center gap-2"><SunMoonIcon class="size-4" /> Tema</DropdownMenu.Label>
							<DropdownMenu.RadioGroup value={userPrefersMode.current} onValueChange={(v) => setMode(v as keyof typeof themes)}>
								{#each Object.entries(themes) as [value, label] (value)}
									<DropdownMenu.RadioItem {value}>{label}</DropdownMenu.RadioItem>
								{/each}
							</DropdownMenu.RadioGroup>
						</DropdownMenu.Group>
						<DropdownMenu.Separator />
						<DropdownMenu.Group>
							<DropdownMenu.Item onclick={onlogout}><LogOutIcon /> Esci</DropdownMenu.Item>
						</DropdownMenu.Group>
					</DropdownMenu.Content>
				</DropdownMenu.Root>
			</Sidebar.MenuItem>
		</Sidebar.Menu>
	</Sidebar.Footer>
</Sidebar.Root>
