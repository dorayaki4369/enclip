import { Num, OpenAPIRoute } from "chanfana";
import { z } from "zod";
import { type AppContext, ClipboardItem } from "../types";

export class ClipboardItemList extends OpenAPIRoute {
	schema = {
		tags: ["Clipboard"],
		summary: "List all Clipboard Items",
        request: {
            query: z.object({
                page: Num({
                    description: "Page number",
                    default: 0,
                    required: false,
                }),
            }),
        },
        responses: {
            "200": {
                description: "Returns a list of clipboard items",
                content: {
                    "application/json": {
                        schema: z.object({
                            data: ClipboardItem.array(),
                        })
                    }
                }
            }
        }
	};

    async handle(c: AppContext) {
        const data = await this.getValidatedData<typeof this.schema>();
        const { page } = data.query;

        return {
            data: [],
        };
    }
}