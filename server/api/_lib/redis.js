// Minimal Upstash Redis REST client. The Vercel Marketplace integration sets
// KV_REST_API_URL / KV_REST_API_TOKEN; a direct Upstash setup uses the UPSTASH_ names.

const url = () => process.env.KV_REST_API_URL ?? process.env.UPSTASH_REDIS_REST_URL;
const token = () => process.env.KV_REST_API_TOKEN ?? process.env.UPSTASH_REDIS_REST_TOKEN;

export const redisConfigured = () => Boolean(url() && token());

export async function redis(...command) {
    const response = await fetch(url(), {
        method: "POST",
        headers: { Authorization: `Bearer ${token()}` },
        body: JSON.stringify(command),
    });
    const data = await response.json();
    if (!response.ok || data.error) throw new Error(`Redis error: ${data.error ?? response.status}`);
    return data.result;
}
