import { redis, redisConfigured } from "../_lib/redis.js";
import {
    KEY,
    MAX_BODY_BYTES,
    MAX_CLOCK_SKEW,
    MIN_INTERVAL,
    parseSnapshot,
    statsConfigured,
    verifySignature,
} from "../_lib/pcStats.js";

// Checks the timestamp and writes in one step, so a replayed or too-frequent update can't sneak in between
const ACCEPT = `
local last = tonumber(redis.call('HGET', KEYS[1], 'ts') or '0')
if tonumber(ARGV[1]) < last + tonumber(ARGV[4]) then return 0 end
redis.call('HSET', KEYS[1], 'ts', ARGV[1], 'data', ARGV[2], 'receivedAt', ARGV[3])
return 1`;

// The agent sends text/plain so Vercel passes along the exact bytes it signed
async function rawBody(req) {
    if (typeof req.body === "string") return req.body;
    if (Buffer.isBuffer(req.body)) return req.body.toString("utf8");
    if (req.body !== undefined) return null; // already parsed into an object, so the signed bytes are gone

    const chunks = [];
    let size = 0;
    for await (const chunk of req) {
        size += chunk.length;
        if (size > MAX_BODY_BYTES) return null;
        chunks.push(chunk);
    }
    return Buffer.concat(chunks).toString("utf8");
}

// The Host header decides which deployment a request reaches, so it can't be swapped to make
// an update signed for another site land here. The app signs the host without a port.
const requestHost = (req) => (req.headers.host ?? "").toLowerCase().replace(/:\d+$/, "");

// Receives signed updates from the pc-stats app
export default async function handler(req, res) {
    res.setHeader("Cache-Control", "no-store");
    if (req.method !== "POST") {
        res.setHeader("Allow", "POST");
        return res.status(405).end();
    }
    if (!statsConfigured() || !redisConfigured()) return res.status(503).end();

    try {
        const body = await rawBody(req);
        if (body === null || Buffer.byteLength(body) > MAX_BODY_BYTES) return res.status(400).end();

        // The signature is checked before the payload is even parsed
        const now = Math.floor(Date.now() / 1000);
        const timestamp = Number(req.headers["x-stats-timestamp"]);
        if (
            !Number.isInteger(timestamp) ||
            Math.abs(now - timestamp) > MAX_CLOCK_SKEW ||
            !verifySignature(requestHost(req), timestamp, body, req.headers["x-stats-signature"])
        ) {
            return res.status(401).end();
        }

        const snapshot = parseSnapshot(body);
        if (!snapshot || snapshot.ts !== timestamp) return res.status(400).end();

        const accepted = await redis(
            "EVAL",
            ACCEPT,
            "1",
            KEY,
            String(timestamp),
            JSON.stringify(snapshot),
            String(now),
            String(MIN_INTERVAL),
        );
        res.status(accepted === 1 ? 204 : 409).end();
    } catch (error) {
        console.error(error);
        res.status(500).end();
    }
}
