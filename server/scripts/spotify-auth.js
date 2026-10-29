// One-time helper that gets the Spotify refresh token the site uses to show what's playing.
//
// 1. Create an app at https://developer.spotify.com/dashboard with the redirect URI
//    http://127.0.0.1:8888/callback (and Web API ticked).
// 2. node scripts/spotify-auth.js <client id> <client secret>
// 3. Log in on the page that opens. The token is printed here; put it in SPOTIFY_REFRESH_TOKEN
//    on Vercel, next to SPOTIFY_CLIENT_ID and SPOTIFY_CLIENT_SECRET.
//
// The token can only read what's currently playing (the user-read-currently-playing scope).

import { spawn } from "node:child_process";
import { randomBytes } from "node:crypto";
import { createServer } from "node:http";

const [clientId, clientSecret] = process.argv.slice(2);
if (!clientId || !clientSecret) {
    console.error("usage: node scripts/spotify-auth.js <client id> <client secret>");
    process.exit(1);
}

const redirectUri = "http://127.0.0.1:8888/callback";
const state = randomBytes(16).toString("hex");
const authorizeUrl = `https://accounts.spotify.com/authorize?${new URLSearchParams({
    client_id: clientId,
    response_type: "code",
    redirect_uri: redirectUri,
    scope: "user-read-currently-playing",
    state,
})}`;

const server = createServer(async (req, res) => {
    const url = new URL(req.url, redirectUri);
    if (url.pathname !== "/callback") return res.writeHead(404).end();

    // The state check makes sure this callback answers the login this script started
    if (url.searchParams.get("state") !== state || !url.searchParams.get("code")) {
        res.writeHead(400).end("Login didn't go through. Run the script again.");
        return;
    }

    const response = await fetch("https://accounts.spotify.com/api/token", {
        method: "POST",
        headers: {
            Authorization: `Basic ${Buffer.from(`${clientId}:${clientSecret}`).toString("base64")}`,
            "Content-Type": "application/x-www-form-urlencoded",
        },
        body: new URLSearchParams({
            grant_type: "authorization_code",
            code: url.searchParams.get("code"),
            redirect_uri: redirectUri,
        }),
    });
    const data = await response.json();

    if (!response.ok || !data.refresh_token) {
        res.writeHead(500).end("Spotify didn't hand over a token. Check the client ID, secret and redirect URI.");
        console.error("Spotify answered:", data);
    } else {
        res.writeHead(200, { "Content-Type": "text/plain" }).end("Done. You can close this tab.");
        console.log(`\nSPOTIFY_REFRESH_TOKEN=${data.refresh_token}\n`);
    }
    server.close();
});

server.listen(8888, "127.0.0.1", () => {
    console.log(`Opening Spotify's login page. If it doesn't open, visit:\n${authorizeUrl}`);
    // `start` is Windows' way to open a URL in the default browser
    spawn("cmd", ["/c", "start", "", authorizeUrl.replaceAll("&", "^&")], { stdio: "ignore", detached: true });
});
