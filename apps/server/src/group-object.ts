/**
 * グループごとの Durable Object。
 * 同じグループへの書き込みはここに集まるので、アイテムの連番が 1 か所で決まる。
 */
import { DurableObject } from "cloudflare:workers";
import { Hono, type Context } from "hono";
import { bearerToken, hashToken, sameHash } from "./auth";
import { GroupStore, type StoredItem } from "./group-store";
import { itemEnvelopeSchema, pairingIdSchema, pairingSlots, type PairingSlot } from "./schemas";

const MAX_ITEMS_PER_PAGE = 100;

/** 欄ごとの権限。参加申請だけはトークンを持たない新端末が書き、ほかは既存端末が書く。 */
const slotAccess: Record<PairingSlot, { write: "public" | "member"; read: "public" | "member" }> = {
	join: { write: "public", read: "member" },
	challenge: { write: "member", read: "public" },
	welcome: { write: "member", read: "public" },
};

export class GroupObject extends DurableObject<Env> {
	private readonly store: GroupStore;
	private readonly app: Hono;

	constructor(ctx: DurableObjectState, env: Env) {
		super(ctx, env);
		this.store = new GroupStore(ctx.storage.sql);
		this.app = this.routes();
	}

	override fetch(request: Request): Promise<Response> {
		return Promise.resolve(this.app.fetch(request));
	}

	override async alarm(): Promise<void> {
		await this.scheduleSweep();
	}

	override webSocketMessage(): void {
		// 端末からの送信は HTTP で受ける。WebSocket は配信専用。
	}

	override webSocketClose(ws: WebSocket, code: number, reason: string): void {
		// 閉じる手順を完了させる。送り返せない予約コードは使わない。
		if (code >= 1000 && code < 5000 && ![1004, 1005, 1006, 1015].includes(code)) {
			ws.close(code, reason);
		} else {
			ws.close();
		}
	}

	private routes(): Hono {
		const app = new Hono().basePath("/groups/:gid");

		app.put("/", async (c) => {
			const token = bearerToken(c.req.raw);
			if (token === null) {
				return c.json({ error: "unauthorized" }, 401);
			}
			const hash = await hashToken(token);
			const existing = this.store.tokenHash();
			if (existing === null) {
				this.store.setTokenHash(hash);
				return c.body(null, 201);
			}
			return sameHash(existing, hash) ? c.body(null, 204) : c.json({ error: "conflict" }, 409);
		});

		app.post("/items", async (c) => {
			const denied = await this.denyNonMember(c);
			if (denied) return denied;
			const parsed = itemEnvelopeSchema.safeParse(await c.req.json().catch(() => null));
			if (!parsed.success) {
				return c.json({ error: "invalid item" }, 400);
			}
			const result = this.store.insertItem(parsed.data);
			if (result.kind === "duplicate") {
				return c.json({ error: "duplicate item" }, 409);
			}
			this.broadcast(result.item);
			await this.scheduleSweep();
			return c.json({ seq: result.item.seq }, 201);
		});

		app.get("/items", async (c) => {
			const denied = await this.denyNonMember(c);
			if (denied) return denied;
			const after = Number(c.req.query("after") ?? "0");
			if (!Number.isSafeInteger(after) || after < 0) {
				return c.json({ error: "invalid cursor" }, 400);
			}
			return c.json({ items: this.store.itemsAfter(after, MAX_ITEMS_PER_PAGE) });
		});

		app.get("/ws", async (c) => {
			const denied = await this.denyNonMember(c);
			if (denied) return denied;
			if (c.req.header("Upgrade") !== "websocket") {
				return c.json({ error: "websocket upgrade required" }, 426);
			}
			const pair = new WebSocketPair();
			this.ctx.acceptWebSocket(pair[1]);
			return new Response(null, { status: 101, webSocket: pair[0] });
		});

		app.put("/pairings/:pid", async (c) => {
			const denied = await this.denyNonMember(c);
			if (denied) return denied;
			const pid = pairingIdSchema.safeParse(c.req.param("pid"));
			if (!pid.success) {
				return c.json({ error: "invalid pairing id" }, 400);
			}
			if (!this.store.createPairing(pid.data, Date.now())) {
				return c.json({ error: "conflict" }, 409);
			}
			await this.scheduleSweep();
			return c.body(null, 201);
		});

		app.delete("/pairings/:pid", async (c) => {
			const denied = await this.denyNonMember(c);
			if (denied) return denied;
			this.store.deletePairing(c.req.param("pid"));
			return c.body(null, 204);
		});

		app.put("/pairings/:pid/:slot", async (c) => {
			const slot = parseSlot(c.req.param("slot"));
			if (slot === null) {
				return c.json({ error: "not found" }, 404);
			}
			if (slotAccess[slot].write === "member") {
				const denied = await this.denyNonMember(c);
				if (denied) return denied;
			} else if (this.store.tokenHash() === null) {
				return c.json({ error: "not found" }, 404);
			}
			const parsed = pairingSlots[slot].safeParse(await c.req.json().catch(() => null));
			if (!parsed.success) {
				return c.json({ error: "invalid message" }, 400);
			}
			switch (this.store.writeSlot(c.req.param("pid"), slot, parsed.data)) {
				case "written":
					return c.body(null, 201);
				case "no_pairing":
					return c.json({ error: "not found" }, 404);
				case "already_written":
					return c.json({ error: "conflict" }, 409);
			}
		});

		app.get("/pairings/:pid/:slot", async (c) => {
			const slot = parseSlot(c.req.param("slot"));
			if (slot === null) {
				return c.json({ error: "not found" }, 404);
			}
			if (slotAccess[slot].read === "member") {
				const denied = await this.denyNonMember(c);
				if (denied) return denied;
			}
			const pid = c.req.param("pid");
			const message = this.store.readSlot(pid, slot);
			if (message === undefined) {
				return c.json({ error: "not found" }, 404);
			}
			if (message === null) {
				return c.body(null, 204);
			}
			if (slot === "welcome") {
				// 新端末が受け取った時点でペアリングは終わる。
				this.store.deletePairing(pid);
			}
			return c.json(message);
		});

		return app;
	}

	/** グループのトークンを持たない要求なら拒否の応答を返す。 */
	private async denyNonMember(c: Context): Promise<Response | null> {
		const expected = this.store.tokenHash();
		if (expected === null) {
			return c.json({ error: "not found" }, 404);
		}
		const token = bearerToken(c.req.raw);
		if (token === null || !sameHash(expected, await hashToken(token))) {
			return c.json({ error: "unauthorized" }, 401);
		}
		return null;
	}

	private broadcast(item: StoredItem): void {
		const message = JSON.stringify({ type: "item", ...item });
		for (const socket of this.ctx.getWebSockets()) {
			try {
				socket.send(message);
			} catch {
				// 切れた接続は close の通知で片付く。端末は再接続時に連番で取り直す。
			}
		}
	}

	private async scheduleSweep(): Promise<void> {
		const next = this.store.sweep(Date.now());
		if (next === null) {
			await this.ctx.storage.deleteAlarm();
		} else {
			await this.ctx.storage.setAlarm(next);
		}
	}
}

function parseSlot(value: string): PairingSlot | null {
	return value in pairingSlots ? (value as PairingSlot) : null;
}
