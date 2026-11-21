import { drizzle } from "drizzle-orm/d1";
import { clipboardItemsTable } from "./schema/clipboard-item";
import { ClipboardItem } from "./types";

function createDB(db: D1Database) {
    return drizzle(db);
}

export function insert(db: D1Database, item: ClipboardItem) {
    const client = createDB(db);

    client.query.clipboardItems.insert({
        content: item.content,
        createdAt: item.createdAt,
    });
}