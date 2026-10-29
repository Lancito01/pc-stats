// Talks to whichever Redis the project has. Vercel's Storage tab offers both kinds:
// - Upstash, reached over its REST API: KV_REST_API_URL / KV_REST_API_TOKEN
//   (or UPSTASH_REDIS_REST_URL / UPSTASH_REDIS_REST_TOKEN when set up on Upstash directly)
// - a regular Redis server, reached over a connection string: REDIS_URL

const restUrl = () => process.env.KV_REST_API_URL ?? process.env.UPSTASH_REDIS_REST_URL;
const restToken = () => process.env.KV_REST_API_TOKEN ?? process.env.UPSTASH_REDIS_REST_TOKEN;

export const redisConfigured = () => Boolean((restUrl() && restToken()) || process.env.REDIS_URL);

async function overRest(command) {
    const response = await fetch(restUrl(), {
        method: "POST",
        headers: { Authorization: `Bearer ${restToken()}` },
        body: JSON.stringify(command),
    });
    const data = await response.json();
    if (!response.ok || data.error) throw new Error(`Redis error: ${data.error ?? response.status}`);
    return data.result;
}

// One connection, reused while the function instance stays warm
let connection;

async function overConnection(command) {
    connection ??= (async () => {
        // Only loaded here, so projects on Upstash don't need the package
        const { createClient } = await import("@redis/client");
        const client = createClient({ url: process.env.REDIS_URL });
        client.on("error", (error) => console.error("Redis connection error:", error));
        await client.connect();
        return client;
    })().catch((error) => {
        connection = undefined; // try again on the next request
        throw error;
    });
    const client = await connection;
    return client.sendCommand(command);
}

// Runs one Redis command, e.g. redis("GET", key). Replies come back the same either way:
// strings, numbers, null, and flat arrays for HGETALL.
export function redis(...command) {
    return restUrl() && restToken() ? overRest(command) : overConnection(command);
}
