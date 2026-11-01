#!/usr/bin/env python3
import sys
import os
import shutil
import sqlite3
import subprocess
import json
import tempfile
import time
import re

try:
    from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes
    from cryptography.hazmat.primitives.kdf.pbkdf2 import PBKDF2HMAC
    from cryptography.hazmat.primitives import hashes
    HAS_CRYPTO = True
except ImportError:
    HAS_CRYPTO = False

def get_secret(app_name):
    try:
        out = subprocess.check_output(['secret-tool', 'lookup', 'application', app_name], stderr=subprocess.DEVNULL)
        if out:
            return out.strip()
    except Exception:
        pass
    return None

def decrypt_v10_v11(enc, raw_pass):
    if not HAS_CRYPTO:
        return None
    kdf = PBKDF2HMAC(algorithm=hashes.SHA1(), length=16, salt=b'saltysalt', iterations=1)
    key = kdf.derive(raw_pass)
    cipher = Cipher(algorithms.AES(key), modes.CBC(b' ' * 16))
    decryptor = cipher.decryptor()
    dec = decryptor.update(enc[3:]) + decryptor.finalize()
    pad = dec[-1]
    plain = dec[:-pad]
    # Check Chromium meta_version 24+ SHA256 prefix (32 bytes)
    if len(plain) > 32 and plain[32:].startswith(b'AQ'):
        return plain[32:].decode('utf-8', errors='ignore')
    elif plain.startswith(b'AQ'):
        return plain.decode('utf-8', errors='ignore')
    s = plain[32:].decode('utf-8', errors='ignore')
    if s.isprintable() and len(s) > 20:
        return s
    return plain.decode('utf-8', errors='ignore')

def scan_cookie_db(db_path, app_name=None):
    if not os.path.exists(db_path):
        return None
    passwords = []
    if app_name:
        sec = get_secret(app_name)
        if sec:
            passwords.append(sec)
    passwords.append(b'peanuts')
    
    with tempfile.NamedTemporaryFile(suffix='.sqlite', delete=False) as tf:
        tmp_db = tf.name
    try:
        shutil.copy2(db_path, tmp_db)
        conn = sqlite3.connect(tmp_db)
        c = conn.cursor()
        c.execute("SELECT name, encrypted_value FROM cookies WHERE host_key LIKE '%spotify%' AND name = 'sp_dc'")
        rows = c.fetchall()
        conn.close()
        for name, enc in rows:
            if not enc:
                continue
            for p in passwords:
                try:
                    val = decrypt_v10_v11(enc, p if isinstance(p, bytes) else p.encode())
                    if val and len(val) > 30 and 'AQ' in val:
                        return val
                except Exception:
                    continue
    except Exception:
        pass
    finally:
        if os.path.exists(tmp_db):
            os.remove(tmp_db)
    return None

def import_from_browsers():
    home = os.path.expanduser('~')
    candidates = [
        ('Brave Browser', os.path.join(home, '.config/BraveSoftware/Brave-Browser/Default/Cookies'), 'brave'),
        ('Brave Browser', os.path.join(home, '.config/BraveSoftware/Brave-Browser/Default/Network/Cookies'), 'brave'),
        ('Google Chrome', os.path.join(home, '.config/google-chrome/Default/Cookies'), 'chrome'),
        ('Google Chrome', os.path.join(home, '.config/google-chrome/Default/Network/Cookies'), 'chrome'),
        ('Chromium', os.path.join(home, '.config/chromium/Default/Cookies'), 'chromium'),
        ('Chromium', os.path.join(home, '.config/chromium/Default/Network/Cookies'), 'chromium'),
    ]
    for name, path, app in candidates:
        sp_dc = scan_cookie_db(path, app)
        if sp_dc:
            return {
                'success': True,
                'sp_dc': sp_dc,
                'source': name,
                'message': f'Successfully imported Spotify credentials from {name}'
            }
    return {
        'success': False,
        'error': 'No Spotify login session found in local browsers (Brave/Chrome/Chromium)'
    }

def find_browser():
    for cmd in ['brave', 'brave-browser', 'google-chrome-stable', 'google-chrome', 'chromium', 'chromium-browser', 'x-www-browser']:
        p = shutil.which(cmd)
        if p:
            return p
    return None

LOGIN_PROFILE_DIR = os.path.expanduser('~/.config/arcy-music/spotify-login-profile')
LOGIN_PID_FILE = os.path.join(LOGIN_PROFILE_DIR, 'login_proc.pid')

def launch_login():
    browser = find_browser()
    if not browser:
        return {'success': False, 'error': 'No supported browser found (Brave, Chrome, or Chromium required)'}
    
    os.makedirs(LOGIN_PROFILE_DIR, exist_ok=True)
    
    # Check if a process is already running
    if os.path.exists(LOGIN_PID_FILE):
        try:
            with open(LOGIN_PID_FILE, 'r') as f:
                old_pid = int(f.read().strip())
            os.kill(old_pid, 0)
            return {'success': True, 'message': 'Spotify login window is already open', 'pid': old_pid}
        except Exception:
            try:
                os.remove(LOGIN_PID_FILE)
            except Exception:
                pass

    # Launch browser window
    display = os.environ.get('DISPLAY', ':0')
    env = dict(os.environ)
    env['DISPLAY'] = display
    
    args = [
        browser,
        '--app=https://accounts.spotify.com/en/login',
        f'--user-data-dir={LOGIN_PROFILE_DIR}',
        '--password-store=basic',
        '--window-size=520,720',
        '--class=arcy-music-spotify-login'
    ]
    
    proc = subprocess.Popen(args, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
    with open(LOGIN_PID_FILE, 'w') as f:
        f.write(str(proc.pid))
        
    return {
        'success': True,
        'message': 'Spotify login window opened. Please log in to complete setup.',
        'pid': proc.pid
    }

def poll_login():
    os.makedirs(LOGIN_PROFILE_DIR, exist_ok=True)
    
    # Check cookies
    cookie_paths = [
        os.path.join(LOGIN_PROFILE_DIR, 'Default/Cookies'),
        os.path.join(LOGIN_PROFILE_DIR, 'Default/Network/Cookies')
    ]
    for cp in cookie_paths:
        sp_dc = scan_cookie_db(cp, None)
        if sp_dc:
            # Terminate login browser if running
            if os.path.exists(LOGIN_PID_FILE):
                try:
                    with open(LOGIN_PID_FILE, 'r') as f:
                        pid = int(f.read().strip())
                    os.kill(pid, 15)
                except Exception:
                    pass
                try:
                    os.remove(LOGIN_PID_FILE)
                except Exception:
                    pass
            return {
                'success': True,
                'in_progress': False,
                'sp_dc': sp_dc,
                'source': 'Spotify Login Window',
                'message': 'Successfully logged in to Spotify!'
            }

    # If no cookie yet, check if process is still running
    if os.path.exists(LOGIN_PID_FILE):
        try:
            with open(LOGIN_PID_FILE, 'r') as f:
                pid = int(f.read().strip())
            os.kill(pid, 0)
            return {
                'success': False,
                'in_progress': True,
                'message': 'Waiting for login in Spotify window...'
            }
        except Exception:
            try:
                os.remove(LOGIN_PID_FILE)
            except Exception:
                pass
            return {
                'success': False,
                'in_progress': False,
                'error': 'Spotify login window was closed before completion'
            }

    return {
        'success': False,
        'in_progress': False,
        'error': 'No active login session'
    }

def sync_spotify(sp_dc=None):
    script_dir = os.path.dirname(os.path.abspath(__file__))
    sync_js = os.path.join(script_dir, 'spotify_sync.js')

    # If sp_dc not provided, try to find in profile or stored cookies
    if not sp_dc:
        cookie_paths = [
            os.path.join(LOGIN_PROFILE_DIR, 'Default/Cookies'),
            os.path.join(LOGIN_PROFILE_DIR, 'Default/Network/Cookies')
        ]
        for cp in cookie_paths:
            found = scan_cookie_db(cp, None)
            if found:
                sp_dc = found
                break

    with tempfile.NamedTemporaryFile(suffix='.json', delete=False) as tf:
        out_json_path = tf.name

    args = ['node', sync_js, '--max-playlists', '100', '--tracks-per-playlist', '10000', '--out', out_json_path]
    if sp_dc:
        args.extend(['--sp-dc', sp_dc])
    if os.path.exists(LOGIN_PROFILE_DIR):
        args.extend(['--profile-dir', LOGIN_PROFILE_DIR])

    try:
        proc = subprocess.run(args, capture_output=True, text=True, timeout=360)
        if not os.path.exists(out_json_path) or os.path.getsize(out_json_path) == 0:
            err = proc.stderr.strip() or proc.stdout.strip() or 'Sync process failed'
            return {'success': False, 'message': err}

        with open(out_json_path, 'r', encoding='utf-8') as f:
            data = json.load(f)

        if not data.get('success'):
            return {'success': False, 'message': data.get('error', 'Spotify sync failed')}

        playlists = data.get('playlists', [])
        db_path = os.path.expanduser('~/.local/share/arcy-music/library.db')
        os.makedirs(os.path.dirname(db_path), exist_ok=True)

        conn = sqlite3.connect(db_path)
        cur = conn.cursor()

        cur.execute("""CREATE TABLE IF NOT EXISTS tracks (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            artist TEXT,
            album TEXT,
            duration_secs INTEGER,
            source_type TEXT NOT NULL,
            source_data TEXT NOT NULL,
            thumbnail_url TEXT
        )""")
        cur.execute("""CREATE TABLE IF NOT EXISTS playlists (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            is_pinned INTEGER NOT NULL DEFAULT 0,
            sort_order INTEGER NOT NULL DEFAULT 0,
            last_played INTEGER NOT NULL DEFAULT 0,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP
        )""")
        cur.execute("""CREATE TABLE IF NOT EXISTS playlist_tracks (
            playlist_id TEXT NOT NULL,
            track_id TEXT NOT NULL,
            position INTEGER NOT NULL,
            PRIMARY KEY (playlist_id, track_id)
        )""")
        cur.execute("""CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        )""")

        total_tracks = 0
        total_playlists = 0

        for pl in playlists:
            pl_id = pl['id']
            pl_name = pl['name']
            is_pinned = pl.get('is_pinned', 0)

            cur.execute("""
                INSERT INTO playlists (id, name, is_pinned, sort_order)
                VALUES (?, ?, ?, 0)
                ON CONFLICT(id) DO UPDATE SET name = excluded.name, is_pinned = excluded.is_pinned
            """, (pl_id, pl_name, is_pinned))
            total_playlists += 1

            for pos, tr in enumerate(pl.get('tracks', [])):
                cur.execute("""
                    INSERT INTO tracks (id, title, artist, album, duration_secs, source_type, source_data, thumbnail_url)
                    VALUES (?, ?, ?, ?, ?, 'spotify', ?, ?)
                    ON CONFLICT(id) DO UPDATE SET
                        title = excluded.title,
                        artist = excluded.artist,
                        album = excluded.album,
                        duration_secs = excluded.duration_secs,
                        thumbnail_url = excluded.thumbnail_url
                """, (
                    tr['id'],
                    tr['title'],
                    tr.get('artist', 'Unknown Artist'),
                    tr.get('album', ''),
                    tr.get('duration_secs', 0),
                    tr.get('spotify_id', tr['id'].replace('sp_', '')),
                    tr.get('thumbnail_url')
                ))

                cur.execute("""
                    INSERT INTO playlist_tracks (playlist_id, track_id, position)
                    VALUES (?, ?, ?)
                    ON CONFLICT(playlist_id, track_id) DO UPDATE SET position = excluded.position
                """, (pl_id, tr['id'], pos))
                total_tracks += 1

        cur.execute("INSERT INTO settings (key, value) VALUES ('spotify_configured', '1') ON CONFLICT(key) DO UPDATE SET value='1'")
        if sp_dc:
            cur.execute("INSERT INTO settings (key, value) VALUES ('spotify_sp_dc', ?) ON CONFLICT(key) DO UPDATE SET value=?", (sp_dc, sp_dc))

        conn.commit()
        conn.close()

        return {
            'success': True,
            'synced_playlists': total_playlists,
            'synced_tracks': total_tracks,
            'playlists_imported': total_playlists,
            'tracks_imported': total_tracks,
            'message': f'Successfully synced {total_playlists} playlists and {total_tracks} tracks from Spotify!'
        }
    except Exception as e:
        return {'success': False, 'message': f'Spotify sync error: {str(e)}'}
    finally:
        if os.path.exists(out_json_path):
            try:
                os.remove(out_json_path)
            except Exception:
                pass

def main():
    if len(sys.argv) < 2:
        print(json.dumps({'error': 'Usage: spotify_helper.py [import-browser|launch-login|poll-login|sync]'}))
        sys.exit(1)
        
    cmd = sys.argv[1]
    if cmd == 'import-browser':
        print(json.dumps(import_from_browsers()))
    elif cmd == 'launch-login':
        print(json.dumps(launch_login()))
    elif cmd == 'poll-login':
        print(json.dumps(poll_login()))
    elif cmd == 'sync':
        sp_dc_arg = sys.argv[2] if len(sys.argv) > 2 else None
        print(json.dumps(sync_spotify(sp_dc_arg)))
    else:
        print(json.dumps({'error': f'Unknown command: {cmd}'}))
        sys.exit(1)

if __name__ == '__main__':
    main()
