<script lang="ts">
	import { Badge } from '$lib/components/ui/badge';

	let { status }: { status: string } = $props();
	const labels: Record<string, string> = {
		active: 'Attivo',
		creating: 'In creazione',
		suspended: 'Sospeso',
		error: 'Errore',
		deleting: 'In eliminazione'
	};
</script>

<Badge variant={status === 'active' ? 'secondary' : status === 'error' ? 'destructive' : 'outline'}>{labels[status] ?? status}</Badge>
