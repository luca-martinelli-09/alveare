// File e cartelle trascinati dal computer: le cartelle si leggono ricorsivamente dalle entry del
// browser (webkitGetAsEntry), ogni file con il percorso relativo ("tema/css/a.css").

/** Il minimo di FileSystemEntry che serve: così si prova anche fuori dal browser. */
export type Entry = {
	isFile: boolean;
	isDirectory: boolean;
	name: string;
	file?: (ok: (f: File) => void, ko?: (e: unknown) => void) => void;
	createReader?: () => { readEntries: (ok: (es: Entry[]) => void, ko?: (e: unknown) => void) => void };
};

export type Dropped = { file: File; path: string };

export async function filesFromEntries(entries: Entry[]): Promise<Dropped[]> {
	const out: Dropped[] = [];
	async function walk(entry: Entry, prefix: string) {
		if (entry.isFile && entry.file) {
			out.push({ file: await new Promise<File>((ok, ko) => entry.file!(ok, ko)), path: prefix + entry.name });
		} else if (entry.isDirectory && entry.createReader) {
			const reader = entry.createReader();
			// readEntries restituisce al massimo ~100 voci per volta: si chiama finché torna vuoto.
			for (;;) {
				const batch = await new Promise<Entry[]>((ok, ko) => reader.readEntries(ok, ko));
				if (!batch.length) break;
				for (const child of batch) await walk(child, `${prefix}${entry.name}/`);
			}
		}
	}
	for (const e of entries) await walk(e, '');
	return out;
}

/** Le entry vanno prese subito, dentro l'evento drop: dopo il primo await il browser le svuota. */
export function entriesOf(data: DataTransfer): Entry[] {
	return [...data.items].map((i) => i.webkitGetAsEntry() as Entry | null).filter((e): e is Entry => e !== null);
}
