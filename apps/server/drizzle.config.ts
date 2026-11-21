import dotenv from "dotenv";
import { defineConfig } from "drizzle-kit";

dotenv.config({
    path: "../../.env",
});

export default defineConfig({
    schema: "./src/schema",
    out: "./drizzle",
    dialect: "sqlite",
    driver: "d1-http",
    dbCredentials: {
        accountId: process.env.CF_ACCOUNT_ID!,
        databaseId: process.env.CF_DATABASE_ID!,
        token: process.env.CF_TOKEN!,
    }
});