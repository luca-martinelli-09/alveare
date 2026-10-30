// WebAuthn nel browser: il pannello manda le opzioni in JSON con i byte in base64url,
// il browser vuole ArrayBuffer (e restituisce ArrayBuffer da riconvertire).
const fromB64 = (s: string) => {
	const b = atob(s.replace(/-/g, '+').replace(/_/g, '/').padEnd(Math.ceil(s.length / 4) * 4, '='));
	return Uint8Array.from(b, (c) => c.charCodeAt(0)).buffer;
};
const toB64 = (buf: ArrayBuffer | null) =>
	buf ? btoa(String.fromCharCode(...new Uint8Array(buf))).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '') : undefined;

type Desc = { id: string; type: string; transports?: string[] };
// eslint-disable-next-line @typescript-eslint/no-explicit-any
type Json = any;

export const passkeysSupported = () => typeof window !== 'undefined' && !!window.PublicKeyCredential;

export async function createPasskey(options: Json) {
	const pk = options.publicKey;
	const cred = (await navigator.credentials.create({
		publicKey: {
			...pk,
			challenge: fromB64(pk.challenge),
			user: { ...pk.user, id: fromB64(pk.user.id) },
			excludeCredentials: (pk.excludeCredentials ?? []).map((c: Desc) => ({ ...c, id: fromB64(c.id) }))
		}
	})) as PublicKeyCredential | null;
	if (!cred) throw new Error('registrazione annullata');
	const r = cred.response as AuthenticatorAttestationResponse;
	return {
		id: cred.id,
		rawId: toB64(cred.rawId),
		type: cred.type,
		response: { attestationObject: toB64(r.attestationObject), clientDataJSON: toB64(r.clientDataJSON), transports: r.getTransports?.() },
		extensions: cred.getClientExtensionResults()
	};
}

export async function usePasskey(options: Json) {
	const pk = options.publicKey;
	const cred = (await navigator.credentials.get({
		publicKey: { ...pk, challenge: fromB64(pk.challenge), allowCredentials: (pk.allowCredentials ?? []).map((c: Desc) => ({ ...c, id: fromB64(c.id) })) }
	})) as PublicKeyCredential | null;
	if (!cred) throw new Error('accesso annullato');
	const r = cred.response as AuthenticatorAssertionResponse;
	return {
		id: cred.id,
		rawId: toB64(cred.rawId),
		type: cred.type,
		response: {
			authenticatorData: toB64(r.authenticatorData),
			clientDataJSON: toB64(r.clientDataJSON),
			signature: toB64(r.signature),
			userHandle: toB64(r.userHandle)
		},
		extensions: cred.getClientExtensionResults()
	};
}
