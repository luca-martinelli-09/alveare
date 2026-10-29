<script lang="ts">
	import * as Table from '$lib/components/ui/table';
	import { api } from '$lib/api';

	let { id }: { id: number } = $props();
	type Entry = { id: number; user_id: number | null; action: string; details: string; at: string };
	let entries = $state<Entry[]>([]);

	$effect(() => {
		api.get<Entry[]>(`/api/sites/${id}/audit`).then((e) => (entries = e));
	});
</script>

<Table.Root class="mt-4">
	<Table.Header>
		<Table.Row>
			<Table.Head>Quando</Table.Head>
			<Table.Head>Utente</Table.Head>
			<Table.Head>Azione</Table.Head>
			<Table.Head>Dettagli</Table.Head>
		</Table.Row>
	</Table.Header>
	<Table.Body>
		{#each entries as e (e.id)}
			<Table.Row>
				<Table.Cell class="whitespace-nowrap">{new Date(e.at).toLocaleString('it-IT')}</Table.Cell>
				<Table.Cell>{e.user_id ?? 'sistema'}</Table.Cell>
				<Table.Cell><code>{e.action}</code></Table.Cell>
				<Table.Cell class="text-muted-foreground">{e.details}</Table.Cell>
			</Table.Row>
		{/each}
	</Table.Body>
</Table.Root>
