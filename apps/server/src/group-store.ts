/**
 * 1 つのグループの保存領域（Durable Object の SQLite）。
 */
import type { ItemEnvelope, PairingSlot } from "./schemas";

/**
 * 端末が終了の連絡をしないまま残ったペアリングを片付けるまでの時間。
 * 有効期限ではなく保存領域の掃除のためで、通常のペアリングはこれより十分短い（仕様 4.1 の 1）。
 */
export const PAIRING_RETENTION_MS = 24 * 60 * 60 * 1000;

export type StoredItem = { seq: number; envelope: ItemEnvelope };

export type ItemInsertResult = { kind: "inserted"; item: StoredItem } | { kind: "duplicate" };

export type SlotWriteResult = "written" | "no_pairing" | "already_written";

export class GroupStore {
	constructor(private readonly sql: SqlStorage) {
		sql.exec(`
			CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
			CREATE TABLE IF NOT EXISTS items (
				seq INTEGER PRIMARY KEY AUTOINCREMENT,
				id TEXT NOT NULL UNIQUE,
				envelope TEXT NOT NULL,
				expires_ms INTEGER NOT NULL
			);
			CREATE TABLE IF NOT EXISTS pairings (
				id TEXT PRIMARY KEY,
				created_ms INTEGER NOT NULL,
				join_msg TEXT,
				challenge_msg TEXT,
				welcome_msg TEXT
			);
		`);
	}

	tokenHash(): string | null {
		const rows = this.sql
			.exec<{ value: string }>("SELECT value FROM meta WHERE key = 'token_hash'")
			.toArray();
		return rows[0]?.value ?? null;
	}

	setTokenHash(hash: string): void {
		this.sql.exec("INSERT INTO meta (key, value) VALUES ('token_hash', ?)", hash);
	}

	insertItem(envelope: ItemEnvelope): ItemInsertResult {
		const existing = this.sql
			.exec("SELECT seq FROM items WHERE id = ?", envelope.id)
			.toArray();
		if (existing.length > 0) {
			return { kind: "duplicate" };
		}
		const row = this.sql
			.exec<{ seq: number }>(
				"INSERT INTO items (id, envelope, expires_ms) VALUES (?, ?, ?) RETURNING seq",
				envelope.id,
				JSON.stringify(envelope),
				envelope.server_expires_ms,
			)
			.one();
		return { kind: "inserted", item: { seq: row.seq, envelope } };
	}

	itemsAfter(seq: number, limit: number): StoredItem[] {
		return this.sql
			.exec<{ seq: number; envelope: string }>(
				"SELECT seq, envelope FROM items WHERE seq > ? ORDER BY seq LIMIT ?",
				seq,
				limit,
			)
			.toArray()
			.map((row) => ({ seq: row.seq, envelope: JSON.parse(row.envelope) as ItemEnvelope }));
	}

	createPairing(id: string, nowMs: number): boolean {
		const existing = this.sql.exec("SELECT id FROM pairings WHERE id = ?", id).toArray();
		if (existing.length > 0) {
			return false;
		}
		this.sql.exec("INSERT INTO pairings (id, created_ms) VALUES (?, ?)", id, nowMs);
		return true;
	}

	deletePairing(id: string): void {
		this.sql.exec("DELETE FROM pairings WHERE id = ?", id);
	}

	/** 各欄は最初の書き込みだけを受け付ける。 */
	writeSlot(id: string, slot: PairingSlot, message: unknown): SlotWriteResult {
		const column = slotColumn(slot);
		const rows = this.sql
			.exec<{ value: string | null }>(`SELECT ${column} AS value FROM pairings WHERE id = ?`, id)
			.toArray();
		const row = rows[0];
		if (row === undefined) {
			return "no_pairing";
		}
		if (row.value !== null) {
			return "already_written";
		}
		this.sql.exec(`UPDATE pairings SET ${column} = ? WHERE id = ?`, JSON.stringify(message), id);
		return "written";
	}

	/** 欄が未記入なら null、ペアリングがなければ undefined。 */
	readSlot(id: string, slot: PairingSlot): unknown | null | undefined {
		const rows = this.sql
			.exec<{ value: string | null }>(
				`SELECT ${slotColumn(slot)} AS value FROM pairings WHERE id = ?`,
				id,
			)
			.toArray();
		const row = rows[0];
		if (row === undefined) {
			return undefined;
		}
		return row.value === null ? null : JSON.parse(row.value);
	}

	/** 期限を過ぎたアイテムと古いペアリングを消し、次に掃除すべき時刻を返す。 */
	sweep(nowMs: number): number | null {
		this.sql.exec("DELETE FROM items WHERE expires_ms <= ?", nowMs);
		this.sql.exec("DELETE FROM pairings WHERE created_ms <= ?", nowMs - PAIRING_RETENTION_MS);
		const next = this.sql
			.exec<{ at: number | null }>(
				`SELECT MIN(at) AS at FROM (
					SELECT MIN(expires_ms) AS at FROM items
					UNION ALL
					SELECT MIN(created_ms) + ? AS at FROM pairings
				)`,
				PAIRING_RETENTION_MS,
			)
			.one();
		return next.at;
	}
}

function slotColumn(slot: PairingSlot): string {
	switch (slot) {
		case "join":
			return "join_msg";
		case "challenge":
			return "challenge_msg";
		case "welcome":
			return "welcome_msg";
	}
}
