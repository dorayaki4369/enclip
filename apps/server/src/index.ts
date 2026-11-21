import { fromHono } from "chanfana";
import { Hono } from "hono";
import { ClipboardItemCreate } from "./endpoints/itemCreate";
import { ClipboardItemList } from "./endpoints/itemList";

// Start a Hono app
const app = new Hono<{ Bindings: Env }>();
app.get("/healthcheck", (c) => new Response(null, { status: 204 }));

// Setup OpenAPI registry
const openapi = fromHono(app, {
	docs_url: "/",
});

// Register OpenAPI endpoints
openapi.post("/api/items", ClipboardItemCreate);
openapi.get("/api/items", ClipboardItemList);

export default app;
