#!/usr/bin/env node
const { spawn } = require('child_process');
const path = require('path');
const os = require('os');
const http = require('http');

const fs = require('fs');

// Parse CLI args
const args = process.argv.slice(2);
function getArg(flag, def) {
  const idx = args.indexOf(flag);
  return idx !== -1 && args[idx + 1] ? args[idx + 1] : def;
}

const profileDir = getArg('--profile-dir', path.join(os.homedir(), '.config/arcy-music/spotify-login-profile'));
const spDc = getArg('--sp-dc', null);
const outFile = getArg('--out', null);
const maxPlaylists = parseInt(getArg('--max-playlists', '100'), 10);
const tracksPerPl = parseInt(getArg('--tracks-per-playlist', '10000'), 10);

function findBrowser() {
  const candidates = ['brave', 'brave-browser', 'google-chrome-stable', 'google-chrome', 'chromium', 'chromium-browser'];
  const { execSync } = require('child_process');
  for (const c of candidates) {
    try {
      execSync(`which ${c}`, { stdio: 'ignore' });
      return c;
    } catch (e) {}
  }
  return 'brave';
}

async function getAvailablePort() {
  return new Promise((resolve) => {
    const srv = http.createServer();
    srv.listen(0, '127.0.0.1', () => {
      const port = srv.address().port;
      srv.close(() => resolve(port));
    });
  });
}

async function main() {
  const browserBin = findBrowser();
  const port = await getAvailablePort();

  const chromeArgs = [
    '--headless=new',
    '--disable-gpu',
    '--no-sandbox',
    '--disable-dev-shm-usage',
    `--user-data-dir=${profileDir}`,
    `--remote-debugging-port=${port}`,
    'https://open.spotify.com/collection/tracks'
  ];

  const proc = spawn(browserBin, chromeArgs, { stdio: 'ignore' });

  const cleanup = () => {
    try { proc.kill('SIGKILL'); } catch (e) {}
  };

  process.on('exit', cleanup);
  process.on('SIGINT', () => { cleanup(); process.exit(1); });
  process.on('SIGTERM', () => { cleanup(); process.exit(1); });

  const startTime = Date.now();
  let ws = null;

  try {
    // Wait for CDP port
    let pageTab = null;
    for (let i = 0; i < 25; i++) {
      await new Promise(r => setTimeout(r, 200));
      try {
        const res = await fetch(`http://127.0.0.1:${port}/json`);
        if (res.ok) {
          const tabs = await res.json();
          pageTab = tabs.find(t => t.type === 'page');
          if (pageTab && pageTab.webSocketDebuggerUrl) break;
        }
      } catch (e) {}
    }

    if (!pageTab || !pageTab.webSocketDebuggerUrl) {
      cleanup();
      console.log(JSON.stringify({ success: false, error: 'Could not connect to browser CDP debug port' }));
      process.exit(1);
    }

    ws = new WebSocket(pageTab.webSocketDebuggerUrl);

    await new Promise((resolve, reject) => {
      let timeout = setTimeout(() => {
        reject(new Error('Timeout waiting for Spotify authentication session'));
      }, 30000);

      ws.onopen = () => {
        ws.send(JSON.stringify({ id: 1, method: 'Network.enable' }));
        if (spDc) {
          const cleanSpDc = spDc.replace(/^sp_dc=/, '').replace(/^"/, '').replace(/"$/, '').trim();
          ws.send(JSON.stringify({
            id: 2,
            method: 'Network.setCookie',
            params: {
              name: 'sp_dc',
              value: cleanSpDc,
              domain: '.spotify.com',
              path: '/',
              secure: true,
              httpOnly: true,
              sameSite: 'Lax'
            }
          }));
        }
      };

      let token = null;
      let clientToken = null;
      let likedSongsUri = null;

      ws.onmessage = async (event) => {
        try {
          const data = JSON.parse(event.data);

          if (data.method === 'Network.requestWillBeSent') {
            const req = data.params.request;
            const post = req.postData;

            if (post && post.includes('fetchPlaylist') && post.includes('37i9dQZF1')) {
              try {
                const parsedPost = JSON.parse(post);
                if (parsedPost.variables?.uri) {
                  likedSongsUri = parsedPost.variables.uri;
                }
              } catch (e) {}
            }

            const auth = req.headers['authorization'] || req.headers['Authorization'];
            const ct = req.headers['client-token'];
            if (auth && auth.startsWith('Bearer ') && !token) {
              token = auth;
              clientToken = ct;
              clearTimeout(timeout);
              timeout = setTimeout(() => {
                reject(new Error('Timeout fetching Spotify playlists and tracks'));
              }, 300000);

              // Give 800ms for liked songs request or page load
              setTimeout(async () => {
                const effectiveLikedUri = likedSongsUri || 'spotify:playlist:37i9dQZF1F5p3rmiWPIYgZ';
                ws.send(JSON.stringify({
                  id: 100,
                  method: 'Runtime.evaluate',
                  params: {
                    expression: `
                      (async () => {
                        // 1. Fetch library playlists with pagination
                        let rawItems = [];
                        let libOffset = 0;
                        const libLimit = 50;

                        while (rawItems.length < ${maxPlaylists}) {
                          try {
                            const libRes = await fetch("https://api-partner.spotify.com/pathfinder/v2/query", {
                              method: "POST",
                              headers: {
                                "content-type": "application/json",
                                "authorization": "${token}",
                                ${clientToken ? `"client-token": "${clientToken}",` : ""}
                                "app-platform": "WebPlayer"
                              },
                              body: JSON.stringify({
                                operationName: "libraryV3",
                                variables: {
                                  filters: [],
                                  order: null,
                                  textFilter: "",
                                  features: ["LIKED_SONGS"],
                                  limit: libLimit,
                                  offset: libOffset,
                                  flatten: false,
                                  expandedFolders: []
                                },
                                extensions: {
                                  persistedQuery: {
                                    version: 1,
                                    sha256Hash: "390c78e5b951029bad359785e69b07b536a509c581cbcd0aded5e5067f187455"
                                  }
                                }
                              })
                            });

                            if (!libRes.ok) break;
                            const libJson = await libRes.json();
                            const pageItems = libJson?.data?.me?.libraryV3?.items || [];
                            if (pageItems.length === 0) break;
                            rawItems.push(...pageItems);
                            libOffset += pageItems.length;
                            if (pageItems.length < libLimit) break;
                          } catch (e) {
                            break;
                          }
                        }

                        const plTargets = [];
                        // Add Liked Songs
                        plTargets.push({
                          uri: "${effectiveLikedUri}",
                          name: "Spotify Liked Songs",
                          is_pinned: 1,
                          id: "spotify_liked"
                        });

                        for (const entry of rawItems) {
                          const pl = entry.item?.data;
                          if (!pl || !pl.uri) continue;
                          if (pl.__typename === "Playlist" && pl.uri.startsWith("spotify:playlist:")) {
                            const pid = pl.uri.replace("spotify:playlist:", "");
                            plTargets.push({
                              uri: pl.uri,
                              name: pl.name || "Untitled Playlist",
                              is_pinned: 0,
                              id: "sp_pl_" + pid
                            });
                          }
                        }

                        // Fetch playlist tracks in parallel batches of 5, paginating each playlist
                        const allPlaylists = [];
                        const batchSize = 5;
                        const targetsToFetch = plTargets.slice(0, ${maxPlaylists});

                        for (let i = 0; i < targetsToFetch.length; i += batchSize) {
                          const batch = targetsToFetch.slice(i, i + batchSize);
                          const batchResults = await Promise.all(batch.map(async (target) => {
                            try {
                              const tracks = [];
                              let offset = 0;
                              const pageSize = 100;
                              let totalCount = Infinity;

                              while (offset < totalCount && tracks.length < ${tracksPerPl}) {
                                const pageLimit = Math.min(pageSize, ${tracksPerPl} - tracks.length);
                                const trRes = await fetch("https://api-partner.spotify.com/pathfinder/v2/query", {
                                  method: "POST",
                                  headers: {
                                    "content-type": "application/json",
                                    "authorization": "${token}",
                                    ${clientToken ? `"client-token": "${clientToken}",` : ""}
                                    "app-platform": "WebPlayer"
                                  },
                                  body: JSON.stringify({
                                    operationName: "fetchPlaylist",
                                    variables: {
                                      uri: target.uri,
                                      offset: offset,
                                      limit: pageLimit,
                                      enableWatchFeedEntrypoint: true
                                    },
                                    extensions: {
                                      persistedQuery: {
                                        version: 1,
                                        sha256Hash: "21ea11a6ac75a5f3e01fd365d29c18e0a4a8b7dc09f89bbb5d73910190c6acaf"
                                      }
                                    }
                                  })
                                });

                                if (!trRes.ok) break;
                                const trJson = await trRes.json();
                                const content = trJson?.data?.playlistV2?.content;
                                if (!content) break;

                                if (typeof content.totalCount === 'number') {
                                  totalCount = content.totalCount;
                                }

                                const trItems = content.items || [];
                                if (trItems.length === 0) break;

                                for (const t of trItems) {
                                  const d = t.itemV2?.data;
                                  if (!d || !d.uri) continue;
                                  const trackId = d.uri.replace("spotify:track:", "");
                                  tracks.push({
                                    id: "sp_" + trackId,
                                    spotify_id: trackId,
                                    title: d.name || "Unknown Track",
                                    artist: d.artists?.items?.map(a => a.profile?.name).filter(Boolean).join(", ") || "Unknown Artist",
                                    album: d.albumOfTrack?.name || "",
                                    duration_secs: Math.round((d.trackDuration?.totalMilliseconds || 0) / 1000),
                                    thumbnail_url: d.albumOfTrack?.coverArt?.sources?.[0]?.url || null
                                  });
                                }

                                offset += trItems.length;
                                if (trItems.length < pageLimit) break;
                              }

                              return {
                                id: target.id,
                                name: target.name,
                                is_pinned: target.is_pinned,
                                tracks
                              };
                            } catch (e) {
                              return null;
                            }
                          }));

                          for (const r of batchResults) {
                            if (r && r.tracks.length > 0) allPlaylists.push(r);
                          }
                        }

                        return allPlaylists;
                      })()
                    `,
                    awaitPromise: true,
                    returnByValue: true
                  }
                }));
              }, 800);
            }
          }

          if (data.id === 100) {
            clearTimeout(timeout);
            const playlists = data.result?.result?.value || [];
            resolve(playlists);
          }
        } catch (err) {
          clearTimeout(timeout);
          reject(err);
        }
      };

      ws.onerror = (e) => {
        clearTimeout(timeout);
        reject(e);
      };
    }).then((playlists) => {
      cleanup();
      const output = JSON.stringify({
        success: true,
        playlists
      });
      if (outFile) {
        fs.writeFileSync(outFile, output, 'utf8');
        console.log(JSON.stringify({ success: true, count: playlists.length, out: outFile }));
        process.exit(0);
      } else {
        process.stdout.write(output + '\n', () => {
          process.exit(0);
        });
      }
    }).catch((err) => {
      cleanup();
      const errOutput = JSON.stringify({
        success: false,
        error: err.message || String(err)
      });
      if (outFile) {
        fs.writeFileSync(outFile, errOutput, 'utf8');
      }
      console.log(errOutput);
      process.exit(1);
    });

  } catch (err) {
    cleanup();
    console.log(JSON.stringify({ success: false, error: err.message || String(err) }));
    process.exit(1);
  }
}

main();
