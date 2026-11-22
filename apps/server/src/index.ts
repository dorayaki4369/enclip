/**
 * enclip の中継サーバーの入口。
 * `/groups/:gid/...` をグループ ID ごとの Durable Object へ渡す。
 */
import { Hono } from "hono";
import { groupIdSchema } from "./schemas";

export { GroupObject } from "./group-object";

const app = new Hono<{ Bindings: Env }>();

app.get("/healthcheck", (c) => c.body(null, 204));

app.all("/groups/:gid/*", (c) => forwardToGroup(c.env, c.req.param("gid"), c.req.raw));
app.all("/groups/:gid", (c) => forwardToGroup(c.env, c.req.param("gid"), c.req.raw));

function forwardToGroup(env: Env, gid: string, request: Request): Promise<Response> | Response {
	if (!groupIdSchema.safeParse(gid).success) {
		return Response.json({ error: "invalid group id" }, { status: 400 });
	}
	return env.GROUPS.getByName(gid).fetch(request);
}

export default app;
