import { OpenAPIRoute } from "chanfana";
import { type AppContext, ClipboardItem } from "../types";
import { z } from "zod";
import { insert } from "../db";

export class ClipboardItemCreate extends OpenAPIRoute {
	schema = {
		tags: ["Clipboard"],
		summary: "Create a new Clipboard Item",
		request: {
			body: {
				content: {
					"application/json": {
						schema: ClipboardItem,
					},
				},
			},
		},
        responses: {
            "201": {
                description: "Returns the created item",
                content: {
                    "application/json": {
                        schema: z.object({
                            result: ClipboardItem,
                        }),
                    }
                }
            }
        }
	};

    async handle(c: AppContext) {
        const data = await this.getValidatedData<typeof this.schema>();
        const itemToCreate = data.body;

        const result = await insert(c.env.DB, itemToCreate);

        return c.json(result, {
            status: 201,
        });
    }
}