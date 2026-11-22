/**
 * グループのアクセス用トークンの扱い。
 * サーバーはトークンそのものを保存せず、SHA-256 のハッシュだけを持つ。
 */
import { accessTokenSchema } from "./schemas";

export async function hashToken(token: string): Promise<string> {
	const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(token));
	return [...new Uint8Array(digest)].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

/** `Authorization: Bearer <token>` からトークンを取り出す。形が合わなければ null。 */
export function bearerToken(request: Request): string | null {
	const header = request.headers.get("Authorization");
	const token = header?.startsWith("Bearer ") ? header.slice("Bearer ".length) : null;
	return token !== null && accessTokenSchema.safeParse(token).success ? token : null;
}

/** ハッシュ同士を長さに依らない時間で比べる。 */
export function sameHash(a: string, b: string): boolean {
	if (a.length !== b.length) {
		return false;
	}
	let diff = 0;
	for (let index = 0; index < a.length; index += 1) {
		diff |= a.charCodeAt(index) ^ b.charCodeAt(index);
	}
	return diff === 0;
}
