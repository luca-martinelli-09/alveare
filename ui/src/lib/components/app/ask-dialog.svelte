<script lang="ts">
	// Il dialogo di askConfirm/askText (vedi $lib/ask.svelte.ts).
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as Field from '$lib/components/ui/field';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { ask } from '$lib/ask.svelte';

	let text = $state('');
	$effect(() => {
		if (ask.current?.kind === 'text') text = ask.current.value ?? '';
	});

	function finish(value: boolean | string | null) {
		const req = ask.current;
		ask.current = null;
		if (req?.kind === 'confirm') req.resolve(value === true);
		else req?.resolve(typeof value === 'string' ? value : null);
	}
</script>

<AlertDialog.Root open={ask.current?.kind === 'confirm'} onOpenChange={(o) => !o && ask.current && finish(false)}>
	<AlertDialog.Content>
		{#if ask.current?.kind === 'confirm'}
			<AlertDialog.Header>
				<AlertDialog.Title>{ask.current.title}</AlertDialog.Title>
				{#if ask.current.description}<AlertDialog.Description>{ask.current.description}</AlertDialog.Description>{/if}
			</AlertDialog.Header>
			<AlertDialog.Footer>
				<AlertDialog.Cancel onclick={() => finish(false)}>Annulla</AlertDialog.Cancel>
				<AlertDialog.Action variant={ask.current.destructive ? 'destructive' : 'primary'} onclick={() => finish(true)}>
					{ask.current.confirmLabel ?? 'Conferma'}
				</AlertDialog.Action>
			</AlertDialog.Footer>
		{/if}
	</AlertDialog.Content>
</AlertDialog.Root>

<Dialog.Root open={ask.current?.kind === 'text'} onOpenChange={(o) => !o && ask.current && finish(null)}>
	<Dialog.Content class="sm:max-w-md">
		{#if ask.current?.kind === 'text'}
			<Dialog.Header>
				<Dialog.Title>{ask.current.title}</Dialog.Title>
				{#if ask.current.description}<Dialog.Description>{ask.current.description}</Dialog.Description>{/if}
			</Dialog.Header>
			<form onsubmit={(e) => (e.preventDefault(), (text.trim() || ask.current?.kind !== 'text' || ask.current.allowEmpty) && finish(text.trim()))}>
				<Field.FieldGroup>
					<Field.Field>
						<Field.FieldLabel for="ask-text">{ask.current.label}</Field.FieldLabel>
						<!-- svelte-ignore a11y_autofocus -->
						<Input id="ask-text" type={ask.current.secret ? 'password' : 'text'} autocomplete={ask.current.secret ? 'current-password' : 'off'} bind:value={text} placeholder={ask.current.placeholder} class={ask.current.mono ? 'font-mono' : ''} autofocus required={!ask.current.allowEmpty} />
					</Field.Field>
					<Dialog.Footer>
						<Button variant="ghost" type="button" onclick={() => finish(null)}>Annulla</Button>
						<Button variant="primary" type="submit">{ask.current.confirmLabel ?? 'Conferma'}</Button>
					</Dialog.Footer>
				</Field.FieldGroup>
			</form>
		{/if}
	</Dialog.Content>
</Dialog.Root>
