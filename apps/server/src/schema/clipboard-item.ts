import { int, sqliteTable, text } from "drizzle-orm/sqlite-core";

export const clipboardItemsTable = sqliteTable('clipboard_items', {
    id: int().primaryKey({ autoIncrement: true }),
    content: text(),
    createdAt: int({ mode: 'timestamp_ms' }),
});