import { redis, redisConfigured } from "../_lib/redis.js";
import { KEY, publicView, statsConfigured } from "../_lib/pcStats.js";
import { nowPlaying, spotifyConfigured } from "../_lib/spotify.js";

async function pcStats(now) {
    // HGETALL answers with a flat [field, value, field, value, ...] list
    const flat = await redis("HGETALL", KEY);
    let record = null;
    for (let i = 0; i < flat.length; i += 2) {
        record ??= {};
        record[flat[i]] = flat[i + 1];
    }
    return publicView(record, now);
}

// One part failing (say, Spotify being down) shouldn't take the other with it
const orNull = (promise) =>
    promise.catch((error) => {
        console.error(error);
        return null;
    });

// The public side: the PC's latest stats, filtered down to what the site shows, and what's
// playing on Spotify. A part that isn't set up is left out; a part that fails comes back null.
export default async function handler(req, res) {
    if (req.method !== "GET") {
        res.setHeader("Allow", "GET");
        return res.status(405).end();
    }

    const now = Date.now();
    const [pc, music] = await Promise.all([
        statsConfigured() && redisConfigured() ? orNull(pcStats(Math.floor(now / 1000))) : undefined,
        spotifyConfigured() ? orNull(nowPlaying()) : undefined,
    ]);

    // Cached at the edge, so visitors share one answer instead of each hitting Redis and Spotify.
    // `at` lets the page move the song's progress bar along from when this was generated.
    res.setHeader("Cache-Control", "public, s-maxage=4, stale-while-revalidate=6");
    res.status(200).json({ at: now, pc, music });
}
