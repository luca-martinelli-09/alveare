#!/usr/bin/env python3
"""Test di integrazione: compose completo, sito WordPress vero, poi eliminazione.

Uso: python3 tests/integration.py  (le immagini alveare/*:dev devono esistere: docker compose build)
Solo libreria standard. Lavora in una cartella temporanea e la ripulisce alla fine.
"""
import base64
import gzip
import hashlib
import http.cookiejar as http_cookiejar
import hmac
import json
import os
import re
import secrets
import shutil
import struct
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
PROJECT = "alveare-it"
NETWORK = "alveare-it-cloudflared"
PORT = 18080
PANEL = "pannello.test"
VISITOR_IP = "198.51.100.9"


def sh(*args, check=True, capture=True, cwd=None):
    r = subprocess.run(args, cwd=cwd, text=True, capture_output=capture)
    if check and r.returncode != 0:
        raise RuntimeError(f"{' '.join(args)}\n{r.stdout}\n{r.stderr}")
    return r.stdout


def totp(secret_b32, t=None):
    key = base64.b32decode(secret_b32 + "=" * (-len(secret_b32) % 8))
    counter = struct.pack(">Q", int((t or time.time()) // 30))
    digest = hmac.new(key, counter, hashlib.sha1).digest()
    off = digest[-1] & 0x0F
    return f"{(struct.unpack('>I', digest[off:off + 4])[0] & 0x7FFFFFFF) % 1_000_000:06d}"


class Http:
    def __init__(self):
        self.cookie = None
        self.token = None

    def call(self, method, path, body=None, host=PANEL, expect=None):
        headers = {"Host": host, "CF-Connecting-IP": VISITOR_IP}
        data = None
        if body is not None:
            data = json.dumps(body).encode()
            headers["Content-Type"] = "application/json"
        if self.token:
            headers["Authorization"] = f"Bearer {self.token}"
        elif self.cookie:
            headers["Cookie"] = self.cookie
            headers["X-Alveare-Csrf"] = "1"
        req = urllib.request.Request(f"http://127.0.0.1:{PORT}{path}", data=data, method=method, headers=headers)
        try:
            with urllib.request.urlopen(req, timeout=60) as r:
                status, raw, hdrs = r.status, r.read(), r.headers
        except urllib.error.HTTPError as e:
            status, raw, hdrs = e.code, e.read(), e.headers
        if hdrs.get("Set-Cookie"):
            self.cookie = hdrs["Set-Cookie"].split(";")[0]
        if expect is not None and status != expect:
            raise AssertionError(f"{method} {path}: atteso {expect}, ricevuto {status}: {raw[:500]!r}")
        try:
            return status, json.loads(raw) if raw else None
        except json.JSONDecodeError:
            return status, raw.decode(errors="replace")


def wait(what, fn, timeout=300, every=2):
    deadline = time.time() + timeout
    last = None
    while time.time() < deadline:
        try:
            v = fn()
            if v:
                return v
        except Exception as e:  # noqa: BLE001 - si riprova fino al timeout
            last = e
        time.sleep(every)
    raise AssertionError(f"timeout: {what} ({last})")


def page(host, path="/"):
    req = urllib.request.Request(f"http://127.0.0.1:{PORT}{path}", headers={"Host": host, "CF-Connecting-IP": VISITOR_IP, "X-Forwarded-Proto": "https"})
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            return r.status, r.read().decode(errors="replace")
    except urllib.error.HTTPError as e:
        return e.code, e.read().decode(errors="replace")


def raw_get(http, path):
    req = urllib.request.Request(f"http://127.0.0.1:{PORT}{path}", headers={"Host": PANEL, "Authorization": f"Bearer {http.token}"})
    with urllib.request.urlopen(req, timeout=120) as r:
        return r.read()


def raw_put(http, path, data):
    req = urllib.request.Request(f"http://127.0.0.1:{PORT}{path}", data=data, method="PUT",
                                 headers={"Host": PANEL, "Authorization": f"Bearer {http.token}", "Content-Type": "application/octet-stream"})
    with urllib.request.urlopen(req, timeout=120) as r:
        return json.loads(r.read())


def run_job(http, method, path, body=None):
    status, resp = http.call(method, path, body, expect=202)
    job = resp["job"]

    def done():
        _, j = http.call("GET", f"/api/jobs/{job}", expect=200)
        return j if j["status"] != "running" else None

    j = wait(f"job {job}", done, timeout=600)
    assert j["status"] == "done", "\n".join(j["log"])
    return j


def main():
    work = tempfile.mkdtemp(prefix="alveare-it-")
    compose = ["docker", "compose", "-p", PROJECT, "-f", "compose.yml", "-f", "compose.test.yml"]
    try:
        shutil.copy(os.path.join(ROOT, "compose.yml"), work)
        shutil.copytree(os.path.join(ROOT, "templates"), os.path.join(work, "templates"))
        with open(os.path.join(work, "compose.test.yml"), "w") as f:
            f.write(f"services:\n  caddy:\n    ports: ['127.0.0.1:{PORT}:80']\n")
        root_pw = secrets.token_hex(16)
        with open(os.path.join(work, ".env"), "w") as f:
            f.write(
                f"ALVEARE_SECRET_KEY={base64.b64encode(os.urandom(32)).decode()}\n"
                f"MARIADB_ROOT_PASSWORD={root_pw}\n"
                f"ALVEARE_ADMIN_EMAIL=admin@example.org\n"
                f"ALVEARE_PANEL_HOST={PANEL}\n"
                f"ALVEARE_ADMINER_HOST=adminer.test\n"
                f"RESTIC_REPOSITORY=/srv/backups/repo\n"
                f"RESTIC_PASSWORD={secrets.token_hex(16)}\n"
                f"ALVEARE_INSECURE_COOKIE=1\n"
                f"ALVEARE_CLOUDFLARED_NETWORK={NETWORK}\n"
            )
        sh("docker", "network", "create", NETWORK, check=False)
        print("avvio dello stack...")
        sh(*compose, "up", "-d", "--no-build", "--wait", cwd=work)

        logs = sh(*compose, "logs", "panel", cwd=work)
        temp = re.search(r"password temporanea (\S+)", logs).group(1)

        http = Http()
        print("primo accesso: password, TOTP, token API")
        _, r = http.call("POST", "/api/auth/login", {"email": "admin@example.org", "password": temp}, expect=200)
        assert r["stage"] == "setup"
        _, setup = http.call("POST", "/api/auth/setup", {"password": "una-password-molto-lunga"}, expect=200)
        http.call("POST", "/api/auth/setup/totp", {"code": totp(setup["secret"])}, expect=200)
        _, me = http.call("GET", "/api/me", expect=200)
        assert me["stage"] == "full"
        _, tok = http.call("POST", "/api/me/tokens", {"name": "integrazione", "days": 1}, expect=201)
        http.token = tok["token"]

        print("creazione di due siti WordPress dal template (scarica il core)...")
        a = run_job(http, "POST", "/api/sites", {"domain": "sona.test", "template": "wordpress", "title": "Sona d'Arco"})
        run_job(http, "POST", "/api/sites", {"domain": "upupa.test", "template": "wordpress", "title": "Upupa"})
        site_a = a["site_id"]

        status, html = page("sona.test")
        assert status == 200 and "wp-content" in html, f"homepage: {status} {html[:300]}"
        assert "Sona d" in html, "titolo del sito non trovato"
        print("homepage di sona.test: 200")

        # Sonde PHP scritte come l'utente del sito: IP reale e isolamento tra siti.
        probe = (
            "<?php echo $_SERVER['REMOTE_ADDR'], '|', posix_getpwuid(posix_geteuid())['name'], '|',"
            " @file_get_contents('/srv/sites/upupa_test/.env') === false ? 'isolato' : 'ESPOSTO', '|',"
            " @file_get_contents('/srv/sites/sona_test/.env') === false ? 'no-env' : 'env-ok';"
        )
        subprocess.run([*compose, "exec", "-T", "php", "sh", "-c",
                        "cat > /srv/sites/sona_test/public/sonda.php && chown sona_test:sona_test /srv/sites/sona_test/public/sonda.php"],
                       input=probe, text=True, cwd=work, check=True)
        _, out = page("sona.test", "/sonda.php")
        ip, user, other, own = out.split("|")
        assert ip == VISITOR_IP, f"REMOTE_ADDR={ip}"
        assert user == "sona_test", f"utente PHP={user}"
        assert other == "isolato", "un sito legge il .env di un altro"
        assert own == "env-ok", "il sito non legge il proprio .env"
        print("IP reale, utente del sito e isolamento: ok")

        access = sh(*compose, "exec", "-T", "caddy", "cat", "/srv/logs/sona_test/access.log", cwd=work)
        assert VISITOR_IP in access, "IP reale assente dall'access log"

        status, _ = page("sona.test", "/wp-content/uploads/x.php")
        assert status == 403, f"php in uploads: {status}"

        print("WP-CLI con l'utente del sito (cron)...")
        sh(*compose, "exec", "-T", "php", "php", "-r",
           "$s=stream_socket_client('unix:///run/alveare/agent.sock');"
           "fwrite($s, json_encode(['cmd'=>'cron_run','slug'=>'sona_test']).\"\\n\");"
           "$r=json_decode(fgets($s), true); exit($r['ok'] ? 0 : 1);", cwd=work)

        print("eliminazione di sona.test...")
        run_job(http, "DELETE", f"/api/sites/{site_a}")
        status, html = page("sona.test")
        assert "wp-content" not in html, "il sito risponde ancora dopo l'eliminazione"
        dbs = sh(*compose, "exec", "-T", "mariadb", "mariadb", "-uroot", f"-p{root_pw}", "-N", "-e", "SHOW DATABASES; SELECT User FROM mysql.user;", cwd=work)
        assert "sona_test" not in dbs and "upupa_test" in dbs, dbs
        pools = sh(*compose, "exec", "-T", "php", "ls", "/srv/php/pools", cwd=work)
        assert "sona_test.conf" not in pools and "upupa_test.conf" in pools, pools
        passwd = sh(*compose, "exec", "-T", "php", "cat", "/etc/passwd", cwd=work)
        assert "sona_test:" not in passwd, "utente Unix rimasto"
        _, alerts = http.call("GET", "/api/alerts", expect=200)
        assert not any(a["kind"] == "backup" for a in alerts), alerts
        trash = sh(*compose, "exec", "-T", "panel", "ls", "/srv/trash", cwd=work)
        assert trash.startswith("sona_test-"), trash
        status, _ = page("upupa.test")
        assert status == 200, "l'altro sito non deve risentirne"

        site_b = next(s["id"] for s in http.call("GET", "/api/sites", expect=200)[1] if s["domain"] == "upupa.test")
        print("database: panoramica, export, reset password")
        _, db = http.call("GET", f"/api/sites/{site_b}/database", expect=200)
        assert any(t["name"] == "wp_posts" for t in db["tables"]), db
        dump = raw_get(http, f"/api/sites/{site_b}/database/export")
        sql = gzip.decompress(dump).decode()
        assert "CREATE TABLE `wp_posts`" in sql and "Dump completed" in sql, sql[-300:]
        http.call("POST", f"/api/sites/{site_b}/database/password", expect=200)
        status, html = page("upupa.test")
        assert status == 200 and "wp-content" in html, "il sito non si collega dopo il cambio password"

        print("Adminer con token monouso")
        _, link = http.call("POST", f"/api/sites/{site_b}/database/adminer", expect=200)
        token = link["url"].split("alveare=")[1]
        jar = http_cookiejar.CookieJar()
        browser = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(jar))
        req = urllib.request.Request(f"http://127.0.0.1:{PORT}/?alveare={token}", headers={"Host": "adminer.test"})
        with browser.open(req, timeout=30) as r:
            body = r.read().decode(errors="replace")
        assert "wp_posts" in body and "upupa_test" in body, body[:800]
        status, _ = page("adminer.test", f"/?alveare={token}")
        assert status == 403, "token riusabile"
        status, _ = page("adminer.test", "/?server=mariadb&username=root")
        assert status == 403, "Adminer accessibile senza token"

        print("backup restic prima dell'import")
        run_job(http, "POST", f"/api/sites/{site_b}/backups")
        _, backups = http.call("GET", f"/api/sites/{site_b}/backups", expect=200)
        snapshot = backups["runs"][0]["snapshot_id"]
        assert backups["runs"][0]["status"] == "done" and snapshot, backups

        print("file manager: upload a blocchi e import del dump")
        data = gzip.compress(sql.replace("Upupa", "Upupa importata").encode())
        _, up = http.call("POST", f"/api/sites/{site_b}/files/uploads", {"path": "import.sql.gz", "size": len(data)}, expect=201)
        chunk = up["chunk_size"]
        for n in range(up["chunks"]):
            raw_put(http, f"/api/sites/{site_b}/files/uploads/{up['id']}/{n}", data[n * chunk:(n + 1) * chunk])
        run_job(http, "POST", f"/api/sites/{site_b}/database/import", {"path": "import.sql.gz"})
        status, html = page("upupa.test")
        assert "Upupa importata" in html, "dump importato non visibile"

        print("restore del solo database dallo snapshot")
        run_job(http, "POST", f"/api/sites/{site_b}/backups/{snapshot[:12]}/restore", {"mode": "database"})
        status, html = page("upupa.test")
        assert status == 200 and "Upupa importata" not in html and "Upupa" in html, "restore non applicato"

        kit = raw_get(http, "/api/admin/recovery-kit").decode()
        assert "RESTIC_PASSWORD=" in kit and "ALVEARE_SECRET_KEY=" in kit

        print("riavvio del pannello: lo stato si rigenera")
        sh(*compose, "restart", "panel", cwd=work)
        wait("pannello di nuovo pronto", lambda: http.call("GET", "/api/sites")[0] == 200, timeout=120)
        status, _ = page("upupa.test")
        assert status == 200

        print("\nINTEGRAZIONE OK")
    finally:
        if os.environ.get("KEEP") != "1":
            subprocess.run([*compose, "down", "-v", "--remove-orphans"], cwd=work, capture_output=True)
            subprocess.run(["docker", "network", "rm", NETWORK], capture_output=True)
            # I dati appartengono a root e agli utenti dei siti: si cancellano da un container.
            subprocess.run(["docker", "run", "--rm", "-v", f"{work}:/w", "debian:trixie-slim", "sh", "-c", "rm -rf /w/* /w/.env"], capture_output=True)
            shutil.rmtree(work, ignore_errors=True)
        else:
            print(f"cartella lasciata: {work}")


if __name__ == "__main__":
    try:
        main()
    except AssertionError as e:
        print(f"\nFALLITO: {e}", file=sys.stderr)
        sys.exit(1)
