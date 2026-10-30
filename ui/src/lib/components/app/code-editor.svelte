<script lang="ts">
	// Editor CodeMirror con evidenziazione scelta dall'estensione del file.
	import { untrack } from 'svelte';
	import { EditorView, basicSetup } from 'codemirror';
	import { php } from '@codemirror/lang-php';
	import { javascript } from '@codemirror/lang-javascript';
	import { css } from '@codemirror/lang-css';
	import { html } from '@codemirror/lang-html';
	import { json } from '@codemirror/lang-json';
	import { StreamLanguage, HighlightStyle, syntaxHighlighting } from '@codemirror/language';
	import { tags as t } from '@lezer/highlight';
	import { properties } from '@codemirror/legacy-modes/mode/properties';

	// class: altezza decisa da chi lo usa (il file manager lo vuole grande, le configurazioni compatte).
	let { value = $bindable(), filename, class: className = 'h-[60vh]' }: { value: string; filename: string; class?: string } = $props();
	let host: HTMLDivElement;

	// Colori dai token del pannello (layout.css): l'editor segue il tema chiaro o scuro.
	const theme = EditorView.theme({
		'&': { backgroundColor: 'var(--background)', color: 'var(--foreground)', height: '100%' },
		'.cm-scroller': { fontFamily: 'var(--font-mono)', lineHeight: '1.6' },
		'.cm-content': { caretColor: 'var(--foreground)' },
		'.cm-cursor, .cm-dropCursor': { borderLeftColor: 'var(--foreground)' },
		'.cm-gutters': { backgroundColor: 'var(--muted)', color: 'var(--muted-foreground)', borderRight: '1px solid var(--border)' },
		'.cm-activeLine': { backgroundColor: 'color-mix(in oklch, var(--muted) 80%, transparent)' },
		'.cm-activeLineGutter': { backgroundColor: 'var(--accent)', color: 'var(--foreground)' },
		'&.cm-focused .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection': {
			backgroundColor: 'color-mix(in oklch, var(--honey-2) 28%, transparent) !important'
		},
		'.cm-selectionMatch': { backgroundColor: 'color-mix(in oklch, var(--honey-2) 14%, transparent)' },
		'&.cm-focused .cm-matchingBracket': { backgroundColor: 'color-mix(in oklch, var(--honey-2) 25%, transparent)', outline: 'none' },
		'.cm-foldPlaceholder': { backgroundColor: 'var(--muted)', border: '1px solid var(--border)', color: 'var(--muted-foreground)' },
		'.cm-tooltip, .cm-panels': { backgroundColor: 'var(--popover)', color: 'var(--popover-foreground)', border: '1px solid var(--border)' },
		'.cm-searchMatch': { backgroundColor: 'color-mix(in oklch, var(--honey-1) 35%, transparent)' }
	});
	const highlight = HighlightStyle.define([
		{ tag: [t.keyword, t.tagName, t.operatorKeyword, t.modifier], color: 'var(--code-keyword)' },
		{ tag: [t.string, t.attributeValue, t.special(t.string)], color: 'var(--code-string)' },
		{ tag: [t.comment, t.meta, t.processingInstruction], color: 'var(--muted-foreground)', fontStyle: 'italic' },
		{ tag: [t.number, t.bool, t.null, t.atom, t.unit], color: 'var(--code-number)' },
		{ tag: [t.attributeName, t.propertyName, t.labelName], color: 'var(--code-attr)' },
		{ tag: [t.function(t.variableName), t.function(t.propertyName)], color: 'var(--code-function)' },
		{ tag: [t.typeName, t.className, t.namespace], color: 'var(--code-type)' },
		{ tag: [t.heading, t.strong], fontWeight: '600' },
		{ tag: t.invalid, color: 'var(--destructive)' }
	]);

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
			// untrack: il contenuto iniziale non deve far ricreare l'editor a ogni tasto (perderebbe il focus).
			doc: untrack(() => value),
			extensions: [
				theme,
				syntaxHighlighting(highlight),
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

<div bind:this={host} class={`overflow-auto rounded-md border text-sm ${className}`}></div>
