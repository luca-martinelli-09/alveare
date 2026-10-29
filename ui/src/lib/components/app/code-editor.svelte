<script lang="ts">
	// Editor CodeMirror con evidenziazione scelta dall'estensione del file.
	import { EditorView, basicSetup } from 'codemirror';
	import { php } from '@codemirror/lang-php';
	import { javascript } from '@codemirror/lang-javascript';
	import { css } from '@codemirror/lang-css';
	import { html } from '@codemirror/lang-html';
	import { json } from '@codemirror/lang-json';
	import { StreamLanguage } from '@codemirror/language';
	import { properties } from '@codemirror/legacy-modes/mode/properties';

	let { value = $bindable(), filename }: { value: string; filename: string } = $props();
	let host: HTMLDivElement;

	function language(name: string) {
		const ext = name.split('.').pop()?.toLowerCase() ?? '';
		if (ext === 'php') return [php()];
		if (['js', 'mjs', 'ts'].includes(ext)) return [javascript({ typescript: ext === 'ts' })];
		if (ext === 'css') return [css()];
		if (['html', 'htm'].includes(ext)) return [html()];
		if (ext === 'json') return [json()];
		if (['ini', 'htaccess', 'env', 'conf'].includes(ext) || name.endsWith('.user.ini')) return [StreamLanguage.define(properties)];
		return [];
	}

	$effect(() => {
		const view = new EditorView({
			parent: host,
			doc: value,
			extensions: [
				basicSetup,
				...language(filename),
				EditorView.updateListener.of((u) => {
					if (u.docChanged) value = u.state.doc.toString();
				})
			]
		});
		return () => view.destroy();
	});
</script>

<div bind:this={host} class="h-[60vh] overflow-auto rounded border text-sm"></div>
