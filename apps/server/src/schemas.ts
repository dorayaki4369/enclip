/**
 * 端末とサーバーの間で受け渡す形。
 * 中身は enclip-core（crates/enclip-core）が作る暗号文や署名で、サーバーは形と大きさだけを確かめる。
 */
import { z } from "zod";

/** base64url（パディングなし）で、復号後のバイト数が `bytes` になる文字列。 */
function fixedB64(bytes: number) {
	return z.string().length(Math.ceil((bytes * 4) / 3)).regex(/^[A-Za-z0-9_-]+$/);
}

/** base64url（パディングなし）で、長さに上限のある文字列。 */
function boundedB64(maxBytes: number) {
	return z
		.string()
		.min(1)
		.max(Math.ceil((maxBytes * 4) / 3))
		.regex(/^[A-Za-z0-9_-]+$/);
}

export const groupIdSchema = fixedB64(16);
export const pairingIdSchema = fixedB64(16);
export const accessTokenSchema = fixedB64(32);

/** クリップボードのテキストを想定した上限。 */
const MAX_ITEM_BODY_BYTES = 256 * 1024;

export const itemEnvelopeSchema = z.object({
	id: fixedB64(16),
	body: boundedB64(MAX_ITEM_BODY_BYTES),
	wrapped_key: boundedB64(128),
	server_expires_ms: z.number().int().nonnegative(),
});
export type ItemEnvelope = z.infer<typeof itemEnvelopeSchema>;

export const joinRequestSchema = z.object({
	joiner: fixedB64(64),
	mac: fixedB64(32),
});

export const challengeSchema = z.object({
	nonce: fixedB64(32),
	signature: fixedB64(64),
});

export const welcomeSchema = z.object({
	encapped: fixedB64(32),
	ciphertext: boundedB64(256),
	signature: fixedB64(64),
});

export const pairingSlots = {
	join: joinRequestSchema,
	challenge: challengeSchema,
	welcome: welcomeSchema,
} as const;
export type PairingSlot = keyof typeof pairingSlots;
