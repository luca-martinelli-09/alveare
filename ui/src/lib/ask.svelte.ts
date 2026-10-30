// Conferme e richieste di testo con i dialoghi di shadcn al posto di confirm() e prompt() del browser.
// Un solo dialogo montato nel layout; chi chiede aspetta la Promise.
type Base = { title: string; description?: string; confirmLabel?: string };
export type ConfirmRequest = Base & { kind: 'confirm'; destructive?: boolean; resolve: (ok: boolean) => void };
export type TextRequest = Base & { kind: 'text'; label: string; value?: string; placeholder?: string; mono?: boolean; allowEmpty?: boolean; resolve: (v: string | null) => void };

export const ask = $state<{ current: ConfirmRequest | TextRequest | null }>({ current: null });

export function askConfirm(o: Omit<ConfirmRequest, 'kind' | 'resolve'>): Promise<boolean> {
	ask.current?.kind === 'confirm' ? ask.current.resolve(false) : ask.current?.resolve(null);
	return new Promise((resolve) => (ask.current = { ...o, kind: 'confirm', resolve }));
}

export function askText(o: Omit<TextRequest, 'kind' | 'resolve'>): Promise<string | null> {
	ask.current?.kind === 'confirm' ? ask.current.resolve(false) : ask.current?.resolve(null);
	return new Promise((resolve) => (ask.current = { ...o, kind: 'text', resolve }));
}
