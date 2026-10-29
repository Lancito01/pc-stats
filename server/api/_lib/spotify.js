import { redis, redisConfigured } from "./redis.js";

// What's playing on the owner's Spotify, from any device. Needs a Spotify app's client ID and
// secret plus a refresh token for the owner's account (see scripts/spotify-auth.js).

const TOKEN_KEY = "spotify:access-token";

export const spotifyConfigured = () =>
    Boolean(process.env.SPOTIFY_CLIENT_ID && process.env.SPOTIFY_CLIENT_SECRET && process.env.SPOTIFY_REFRESH_TOKEN);

let memoryToken; // used when Redis isn't set up: { value, expiresAt }

async function accessToken() {
    if (redisConfigured()) {
        const cached = await redis("GET", TOKEN_KEY);
        if (cached) return cached;
    } else if (memoryToken && memoryToken.expiresAt > Date.now()) {
        return memoryToken.value;
    }

    const credentials = Buffer.from(`${process.env.SPOTIFY_CLIENT_ID}:${process.env.SPOTIFY_CLIENT_SECRET}`).toString("base64");
    const response = await fetch("https://accounts.spotify.com/api/token", {
        method: "POST",
        headers: { Authorization: `Basic ${credentials}`, "Content-Type": "application/x-www-form-urlencoded" },
        body: new URLSearchParams({ grant_type: "refresh_token", refresh_token: process.env.SPOTIFY_REFRESH_TOKEN }),
    });
    if (!response.ok) throw new Error(`Spotify token refresh failed with ${response.status}`);
    const { access_token: value, expires_in: expiresIn } = await response.json();

    // Spotify tokens last an hour; drop ours a minute early
    const ttl = Math.max(60, expiresIn - 60);
    if (redisConfigured()) await redis("SET", TOKEN_KEY, value, "EX", String(ttl));
    else memoryToken = { value, expiresAt: Date.now() + ttl * 1000 };
    return value;
}

// Returns the track or episode that's playing right now, or null when nothing is (paused counts as nothing)
export async function nowPlaying() {
    const response = await fetch("https://api.spotify.com/v1/me/player/currently-playing?additional_types=episode", {
        headers: { Authorization: `Bearer ${await accessToken()}` },
    });
    if (response.status === 204) return null;
    if (!response.ok) throw new Error(`Spotify responded with ${response.status}`);

    const data = await response.json();
    const item = data.item;
    if (!data.is_playing || !item) return null;

    const isEpisode = item.type === "episode";
    const images = (isEpisode ? item.images ?? item.show?.images : item.album?.images) ?? [];
    // Spotify lists images largest first; the widget only needs a small one
    const image = images.findLast((img) => (img.width ?? 0) >= 64) ?? images[0];

    return {
        title: item.name,
        artist: isEpisode ? item.show?.name : item.artists?.map((artist) => artist.name).join(", "),
        image: image?.url ?? null,
        url: item.external_urls?.spotify ?? null,
        progressMs: data.progress_ms ?? 0,
        durationMs: item.duration_ms ?? 0,
    };
}
