import { createPublicKey, verify } from "node:crypto";

// Live stats sent by the pc-stats app (https://github.com/Lancito01/pc-stats). The app signs
// every update with a private key that never leaves that PC; this server only holds the public
// half (STATS_PUBLIC_KEY), so nobody can post fake stats, even with access to its environment.

export const KEY = "pc-stats:latest";
export const MAX_BODY_BYTES = 2048;
export const MAX_CLOCK_SKEW = 120; // seconds the PC's clock may be off from the server's
export const MIN_INTERVAL = 4; // seconds between accepted updates (the app sends every 5s at most)
export const ONLINE_WINDOW = 150; // no update for this long means offline (the app sends at least once a minute)
export const AWAY_AFTER = 300; // seconds without keyboard or mouse input before showing "away"

// What the site serves. The agent can send more, but anything not listed here stays private.
export const PUBLIC_FIELDS = [
    "interval",
    "uptime",
    "cpu",
    "gpu",
    "ramUsed",
    "ramTotal",
    "clicks",
    "keys",
    "scrolls",
    "mouse",
    "app",
    "appInfo",
];

// The prefix keeps a signature from being valid for anything else the key might sign, the
// host keeps an update meant for one site from being replayed against another, and the
// timestamp ties each payload to the moment it was sent.
export const signedMessage = (host, timestamp, body) => `pc-stats/v1\n${host}\n${timestamp}\n${body}`;

export const statsConfigured = () => Boolean(process.env.STATS_PUBLIC_KEY);

let publicKey;

function loadPublicKey() {
    if (!publicKey) {
        const key = createPublicKey({ key: JSON.parse(process.env.STATS_PUBLIC_KEY), format: "jwk" });
        if (key.asymmetricKeyDetails?.namedCurve !== "prime256v1") throw new Error("STATS_PUBLIC_KEY must be a P-256 key");
        publicKey = key;
    }
    return publicKey;
}

export function verifySignature(host, timestamp, body, signature) {
    // A P-256 signature is 64 bytes, so exactly 88 base64 characters
    if (typeof signature !== "string" || !/^[A-Za-z0-9+/]{86}==$/.test(signature)) return false;
    return verify(
        "sha256",
        Buffer.from(signedMessage(host, timestamp, body)),
        // .NET signs in IEEE P1363 form (r || s) rather than DER
        { key: loadPublicKey(), dsaEncoding: "ieee-p1363" },
        Buffer.from(signature, "base64"),
    );
}

const int = (min, max) => (v) => Number.isInteger(v) && v >= min && v <= max;
const num = (min, max) => (v) => typeof v === "number" && Number.isFinite(v) && v >= min && v <= max;
// Letters, numbers, punctuation, symbols and spaces only: no control characters or line breaks
const text = (max) => (v) => typeof v === "string" && v.length <= max && /^[\p{L}\p{N}\p{P}\p{S} ]+$/u.test(v);
const YEAR = 365 * 24 * 60 * 60;

// Every field the agent may send, with its allowed range. Unknown fields fail the whole update.
const FIELDS = {
    v: (v) => v === 1,
    ts: int(1_600_000_000, 4_000_000_000),
    interval: int(5, 600), // seconds until the app's next update, so the site can glide between the two
    uptime: int(0, 10 * YEAR), // seconds since boot
    idle: int(0, 10 * YEAR), // seconds since the last keyboard or mouse input
    cpu: int(0, 100), // percent
    gpu: int(0, 100), // percent
    ramUsed: num(0, 4096), // GB
    ramTotal: num(0, 4096), // GB
    day: (v) => typeof v === "string" && /^\d{4}-\d{2}-\d{2}$/.test(v), // the PC's local date the counts below belong to
    clicks: int(0, 1e9),
    keys: int(0, 1e9),
    scrolls: int(0, 1e9),
    mouse: num(0, 1e7), // metres
    // Only sent while an app from the agent's own list is focused; both strings come from that list
    app: text(40), // e.g. "VS Code"
    appInfo: text(80), // e.g. "writing code"
};

export function parseSnapshot(body) {
    let data;
    try {
        data = JSON.parse(body);
    } catch {
        return null;
    }
    if (typeof data !== "object" || data === null || Array.isArray(data)) return null;
    if (!("v" in data) || !("ts" in data)) return null;
    for (const [field, value] of Object.entries(data)) {
        if (!Object.hasOwn(FIELDS, field) || !FIELDS[field](value)) return null;
    }
    return data;
}

// record is the stored hash: { ts, data, receivedAt }, or null before the first update.
// Once the PC goes quiet, nothing about it is served, not even when it was last seen.
export function publicView(record, now) {
    if (!record || now - Number(record.receivedAt) > ONLINE_WINDOW) return { online: false };

    const snapshot = JSON.parse(record.data);
    const view = { online: true, status: snapshot.idle >= AWAY_AFTER ? "away" : "active" };
    for (const field of PUBLIC_FIELDS) {
        if (field in snapshot) view[field] = snapshot[field];
    }
    return view;
}
