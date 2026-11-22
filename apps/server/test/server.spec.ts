import { exports } from "cloudflare:workers";
import { describe, expect, it } from "vitest";

const BASE = "https://enclip.test";

function randomB64(bytes: number): string {
	const data = crypto.getRandomValues(new Uint8Array(bytes));
	return btoa(String.fromCharCode(...data))
		.replaceAll("+", "-")
		.replaceAll("/", "_")
		.replace(/=+$/, "");
}

function call(path: string, init: RequestInit & { token?: string } = {}): Promise<Response> {
	const headers = new Headers(init.headers);
	if (init.token !== undefined) {
		headers.set("Authorization", `Bearer ${init.token}`);
	}
	if (init.body !== undefined) {
		headers.set("Content-Type", "application/json");
	}
	return exports.default.fetch(`${BASE}${path}`, { ...init, headers });
}

async function createGroup(): Promise<{ gid: string; token: string }> {
	const gid = randomB64(16);
	const token = randomB64(32);
	const response = await call(`/groups/${gid}`, { method: "PUT", token });
	expect(response.status).toBe(201);
	return { gid, token };
}

function envelope(expiresMs = Date.now() + 60_000) {
	return {
		id: randomB64(16),
		body: randomB64(64),
		wrapped_key: randomB64(60),
		server_expires_ms: expiresMs,
	};
}

describe("groups", () => {
	it("rejects a malformed group id", async () => {
		const response = await call("/groups/not-a-group-id/items");
		expect(response.status).toBe(400);
	});

	it("is idempotent for the same token and rejects another token", async () => {
		const { gid, token } = await createGroup();
		expect((await call(`/groups/${gid}`, { method: "PUT", token })).status).toBe(204);
		expect((await call(`/groups/${gid}`, { method: "PUT", token: randomB64(32) })).status).toBe(409);
	});
});

describe("items", () => {
	it("stores items in order and returns them after a cursor", async () => {
		const { gid, token } = await createGroup();
		const first = envelope();
		const second = envelope();
		const firstResponse = await call(`/groups/${gid}/items`, {
			method: "POST",
			token,
			body: JSON.stringify(first),
		});
		expect(firstResponse.status).toBe(201);
		const { seq } = (await firstResponse.json()) as { seq: number };
		await call(`/groups/${gid}/items`, { method: "POST", token, body: JSON.stringify(second) });

		const all = (await (await call(`/groups/${gid}/items`, { token })).json()) as {
			items: { seq: number; envelope: unknown }[];
		};
		expect(all.items.map((item) => item.envelope)).toEqual([first, second]);

		const after = (await (await call(`/groups/${gid}/items?after=${seq}`, { token })).json()) as {
			items: { envelope: unknown }[];
		};
		expect(after.items.map((item) => item.envelope)).toEqual([second]);
	});

	it("rejects requests without the group token", async () => {
		const { gid } = await createGroup();
		expect((await call(`/groups/${gid}/items`)).status).toBe(401);
		expect((await call(`/groups/${gid}/items`, { token: randomB64(32) })).status).toBe(401);
		const post = await call(`/groups/${gid}/items`, {
			method: "POST",
			token: randomB64(32),
			body: JSON.stringify(envelope()),
		});
		expect(post.status).toBe(401);
	});

	it("returns not found for an unknown group", async () => {
		expect((await call(`/groups/${randomB64(16)}/items`, { token: randomB64(32) })).status).toBe(404);
	});

	it("rejects duplicate ids and malformed envelopes", async () => {
		const { gid, token } = await createGroup();
		const item = envelope();
		await call(`/groups/${gid}/items`, { method: "POST", token, body: JSON.stringify(item) });
		const duplicate = await call(`/groups/${gid}/items`, {
			method: "POST",
			token,
			body: JSON.stringify(item),
		});
		expect(duplicate.status).toBe(409);
		const malformed = await call(`/groups/${gid}/items`, {
			method: "POST",
			token,
			body: JSON.stringify({ ...envelope(), id: "short" }),
		});
		expect(malformed.status).toBe(400);
	});

	it("pushes new items to connected WebSockets", async () => {
		const { gid, token } = await createGroup();
		const response = await call(`/groups/${gid}/ws`, { token, headers: { Upgrade: "websocket" } });
		expect(response.status).toBe(101);
		const socket = response.webSocket;
		if (socket === null) throw new Error("no websocket");
		socket.accept();
		const received = new Promise<string>((resolve) => {
			socket.addEventListener("message", (event) => resolve(event.data as string));
		});
		const item = envelope();
		await call(`/groups/${gid}/items`, { method: "POST", token, body: JSON.stringify(item) });
		const message = JSON.parse(await received) as { type: string; envelope: unknown };
		expect(message.type).toBe("item");
		expect(message.envelope).toEqual(item);
		socket.close();
	});
});

describe("pairings", () => {
	const join = () => ({ joiner: randomB64(64), mac: randomB64(32) });
	const challenge = () => ({ nonce: randomB64(32), signature: randomB64(64) });
	const welcome = () => ({ encapped: randomB64(32), ciphertext: randomB64(80), signature: randomB64(64) });

	it("relays join, challenge and welcome between devices", async () => {
		const { gid, token } = await createGroup();
		const pid = randomB64(16);
		const base = `/groups/${gid}/pairings/${pid}`;
		expect((await call(base, { method: "PUT", token })).status).toBe(201);

		// 新端末はトークンなしで参加申請を書く。
		const joinMessage = join();
		expect((await call(`${base}/join`, { method: "PUT", body: JSON.stringify(joinMessage) })).status).toBe(201);
		// 参加申請を読めるのはグループの端末だけ。
		expect((await call(`${base}/join`)).status).toBe(401);
		expect(await (await call(`${base}/join`, { token })).json()).toEqual(joinMessage);

		// 新端末はトークンなしで承認用ノンスと welcome を読む。
		expect((await call(`${base}/challenge`)).status).toBe(204);
		const challengeMessage = challenge();
		expect(
			(await call(`${base}/challenge`, { method: "PUT", body: JSON.stringify(challengeMessage) })).status,
		).toBe(401);
		await call(`${base}/challenge`, { method: "PUT", token, body: JSON.stringify(challengeMessage) });
		expect(await (await call(`${base}/challenge`)).json()).toEqual(challengeMessage);

		const welcomeMessage = welcome();
		await call(`${base}/welcome`, { method: "PUT", token, body: JSON.stringify(welcomeMessage) });
		expect(await (await call(`${base}/welcome`)).json()).toEqual(welcomeMessage);
		// welcome を受け取った時点でペアリングは終わる。
		expect((await call(`${base}/welcome`)).status).toBe(404);
	});

	it("accepts only the first join request", async () => {
		const { gid, token } = await createGroup();
		const base = `/groups/${gid}/pairings/${randomB64(16)}`;
		await call(base, { method: "PUT", token });
		const first = join();
		await call(`${base}/join`, { method: "PUT", body: JSON.stringify(first) });
		expect((await call(`${base}/join`, { method: "PUT", body: JSON.stringify(join()) })).status).toBe(409);
		expect(await (await call(`${base}/join`, { token })).json()).toEqual(first);
	});

	it("rejects join requests for unknown or deleted pairings", async () => {
		const { gid, token } = await createGroup();
		const base = `/groups/${gid}/pairings/${randomB64(16)}`;
		expect((await call(`${base}/join`, { method: "PUT", body: JSON.stringify(join()) })).status).toBe(404);
		await call(base, { method: "PUT", token });
		await call(base, { method: "DELETE", token });
		expect((await call(`${base}/join`, { method: "PUT", body: JSON.stringify(join()) })).status).toBe(404);
	});
});
