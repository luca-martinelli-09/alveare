#!/usr/bin/env python3
"""Test di integrazione: compose completo, sito WordPress vero, poi eliminazione.

Uso: python3 tests/integration.py  (le immagini alveare/*:dev devono esistere: docker compose build)
Solo libreria standard. Lavora in una cartella temporanea e la ripulisce alla fine.
"""
import base64
import gzip
import io
import tarfile
import zipfile
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
PANEL = "pannello.localhost"
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


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *args, **kwargs):
        return None


def page(host, path="/", auth=None, headers=False):
    """(status, html) della pagina; con `headers=True` anche gli header. I redirect non si seguono."""
    h = {"Host": host, "CF-Connecting-IP": VISITOR_IP, "X-Forwarded-Proto": "https"}
    if auth:
        h["Authorization"] = "Basic " + base64.b64encode(f"{auth[0]}:{auth[1]}".encode()).decode()
    req = urllib.request.Request(f"http://127.0.0.1:{PORT}{path}", headers=h)
    try:
        with urllib.request.build_opener(NoRedirect).open(req, timeout=30) as r:
            out = (r.status, r.read().decode(errors="replace"), r.headers)
    except urllib.error.HTTPError as e:
        out = (e.code, e.read().decode(errors="replace"), e.headers)
    return out if headers else out[:2]


def raw_get(http, path):
    req = urllib.request.Request(f"http://127.0.0.1:{PORT}{path}", headers={"Host": PANEL, "Authorization": f"Bearer {http.token}"})
    with urllib.request.urlopen(req, timeout=120) as r:
        return r.read()


def raw_put(http, path, data):
    req = urllib.request.Request(f"http://127.0.0.1:{PORT}{path}", data=data, method="PUT",
                                 headers={"Host": PANEL, "Authorization": f"Bearer {http.token}", "Content-Type": "application/octet-stream"})
    with urllib.request.urlopen(req, timeout=120) as r:
        return json.loads(r.read())


def agent(compose, work, cmd):
    """Un comando all'agente PHP, come lo manda il pannello."""
    php = ("$s=stream_socket_client('unix:///run/alveare/agent.sock');"
           "fwrite($s, $argv[1].\"\\n\"); $r=json_decode(fgets($s), true);"
           "echo $r['output']; exit($r['ok'] ? 0 : 1);")
    return sh(*compose, "exec", "-T", "php", "php", "-r", php, json.dumps(cmd), cwd=work)


def read_env(work, slug, key):
    text = sh("docker", "compose", "-p", PROJECT, "exec", "-T", "panel", "cat", f"/srv/sites/{slug}/.env", cwd=work)
    return re.search(rf"^{key}='([^']*)'", text, re.M).group(1)


def upload_import(http, name, data):
    _, up = http.call("POST", "/api/imports/uploads", {"path": name, "size": len(data)}, expect=201)
    for n in range(up["chunks"]):
        raw_put(http, f"/api/imports/uploads/{up['id']}/{n}", data[n * up["chunk_size"]:(n + 1) * up["chunk_size"]])


def agent_rt(compose, work, runtime, cmd):
    """Come agent(), ma verso l'agente del container di un runtime PHP aggiuntivo."""
    php = ("$s=stream_socket_client('unix:///run/alveare/agent-" + runtime + ".sock');"
           "fwrite($s, $argv[1].\"\\n\"); $r=json_decode(fgets($s), true);"
           "echo $r['output']; exit($r['ok'] ? 0 : 1);")
    return sh(*compose, "exec", "-T", "php84", "php", "-r", php, json.dumps(cmd), cwd=work)


def wp_home(compose, work, slug):
    return agent(compose, work, {"cmd": "wp", "slug": slug, "args": ["option", "get", "home"]}).strip()


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
    # Profili: worker e runtime PHP 8.4, per provare anche i container opzionali.
    compose = ["docker", "compose", "-p", PROJECT, "-f", "compose.yml", "-f", "compose.test.yml", "--profile", "workers", "--profile", "php84"]
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
                f"ALVEARE_PHP_RUNTIMES=8.4\n"
                f"ALVEARE_METRICS_TOKEN=prometheus-prova\n"
                f"ALVEARE_CLOUDFLARED_NETWORK={NETWORK}\n"
            )
        sh("docker", "network", "create", NETWORK, check=False)
        print("avvio dello stack...")
        sh(*compose, "up", "-d", "--no-build", "--wait", cwd=work)

        logs = sh(*compose, "logs", "panel", cwd=work)
        temp = re.search(r"password temporanea (\S+)", logs).group(1)

        http = Http()
        status, html = page(PANEL, "/sites/1")
        assert status == 200 and "/_app/" in html, f"interfaccia non servita: {status}"

        print("primo accesso: password, TOTP, token API")
        _, r = http.call("POST", "/api/auth/login", {"email": "admin@example.org", "password": temp}, expect=200)
        assert r["stage"] == "setup"
        _, setup = http.call("POST", "/api/auth/setup", {"password": "una-password-molto-lunga"}, expect=200)
        http.call("POST", "/api/auth/setup/totp", {"code": totp(setup["secret"])}, expect=200)
        _, me = http.call("GET", "/api/me", expect=200)
        assert me["stage"] == "full"
        _, tok = http.call("POST", "/api/me/tokens", {"name": "integrazione", "days": 1}, expect=201)
        http.token = tok["token"]
        # Per i test nel browser (ui/e2e) sullo stesso stack: E2E_CONFIG=<cartella>/e2e.json npm run e2e
        with open(os.path.join(work, "e2e.json"), "w") as f:
            json.dump({"url": f"http://{PANEL}:{PORT}", "email": "admin@example.org", "password": "una-password-molto-lunga", "totp_secret": setup["secret"]}, f)
        if os.environ.get("E2E") == "1":
            print("test nel browser (Playwright)")
            # Nell'immagine ufficiale di Playwright: browser giusti su qualunque distribuzione.
            subprocess.run(["docker", "run", "--rm", "--network", "host", "-v", f"{os.path.join(ROOT, 'ui')}:/ui", "-v", f"{work}/e2e.json:/e2e.json:ro",
                            "-w", "/ui", "-e", "E2E_CONFIG=/e2e.json", "mcr.microsoft.com/playwright:v1.56.1-noble", "npx", "playwright", "test"], check=True)

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

        reach = subprocess.run([*compose, "exec", "-T", "php", "php", "-r",
                                "echo @file_get_contents('http://panel:8080/healthz') === false ? 'no' : 'si';"],
                               cwd=work, capture_output=True, text=True).stdout
        assert reach == "no", "il codice PHP raggiunge il pannello"

        status, _ = page("sona.test", "/wp-content/uploads/x.php")
        assert status == 403, f"php in uploads: {status}"

        print("Redis: object cache con l'utente ACL del sito")
        agent(compose, work, {"cmd": "wp", "slug": "upupa_test", "args": ["plugin", "activate", "redis-cache"]})
        # Drop-in copiato a mano: `wp redis enable` e `update-dropin` fanno FLUSHDB, negato dall'ACL.
        agent(compose, work, {"cmd": "wp", "slug": "upupa_test", "args": ["eval", "copy(WP_PLUGIN_DIR . '/redis-cache/includes/object-cache.php', WP_CONTENT_DIR . '/object-cache.php');"]})
        status = agent(compose, work, {"cmd": "wp", "slug": "upupa_test", "args": ["redis", "status"]})
        assert "Connected" in status, status
        code, _ = page("upupa.test")
        assert code == 200
        keys = sh(*compose, "exec", "-T", "redis", "redis-cli", "--user", "upupa_test", "--pass", read_env(work, "upupa_test", "REDIS_PASSWORD"), "--no-auth-warning", "SCAN", "0", "COUNT", "1000", cwd=work)
        assert "upupa_test:" in keys, keys
        denied = sh(*compose, "exec", "-T", "redis", "redis-cli", "--user", "upupa_test", "--pass", read_env(work, "upupa_test", "REDIS_PASSWORD"), "--no-auth-warning", "GET", "altro:chiave", cwd=work)
        assert "NOPERM" in denied, denied

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


        print("fase 2: sospensione, staging e domini")
        http.call("PATCH", f"/api/sites/{site_b}", {"suspended": True}, expect=200)
        status, html, hdrs = page("upupa.test", headers=True)
        assert status == 503 and "manutenzione" in html and hdrs["Retry-After"] == "3600", (status, html[:200])
        http.call("PATCH", f"/api/sites/{site_b}", {"suspended": False}, expect=200)
        wait("sito riattivato", lambda: page("upupa.test")[0] == 200, timeout=30)
        _, st = http.call("PATCH", f"/api/sites/{site_b}", {"staging": True}, expect=200)
        creds = (st["staging"]["user"], st["staging"]["password"])
        assert page("upupa.test")[0] == 401, "staging senza password"
        status, _, hdrs = page("upupa.test", auth=creds, headers=True)
        assert status == 200 and hdrs["X-Robots-Tag"] == "noindex, nofollow", (status, dict(hdrs))
        http.call("PATCH", f"/api/sites/{site_b}", {"staging": False}, expect=200)
        assert page("upupa.test")[0] == 200
        http.call("POST", f"/api/sites/{site_b}/domains", {"hostname": "www.upupa.test", "kind": "redirect"}, expect=201)
        status, _, hdrs = page("www.upupa.test", "/chi-siamo/", headers=True)
        assert status == 301 and hdrs["Location"] == "https://upupa.test/chi-siamo/", (status, dict(hdrs))

        print("clona upupa.test su copia.test")
        clone = run_job(http, "POST", f"/api/sites/{site_b}/clone", {"domain": "copia.test"})
        _, cdetail = http.call("GET", f"/api/sites/{clone['site_id']}", expect=200)
        ccreds = (cdetail["staging"]["user"], cdetail["staging"]["password"])
        assert page("copia.test")[0] == 401, "la copia deve nascere protetta"
        status, html = page("copia.test", auth=ccreds)
        assert status == 200 and "Upupa" in html and "copia.test" in html and "//upupa.test" not in html, html[:500]
        assert wp_home(compose, work, "copia_test") == "https://copia.test"

        print("esporta upupa.test")
        run_job(http, "POST", f"/api/sites/{site_b}/export")
        _, exports = http.call("GET", f"/api/sites/{site_b}/exports", expect=200)
        archive = tarfile.open(fileobj=io.BytesIO(raw_get(http, f"/api/exports/{exports[0]['token']}")), mode="r:gz")
        names = archive.getnames()
        for needed in ["upupa_test/public/wp-config.php", "upupa_test/database.sql", "upupa_test/LEGGIMI.txt", "upupa_test/.env.example"]:
            assert needed in names, f"manca {needed}"
        assert "upupa_test/.env" not in names, "il .env non deve uscire"
        example = archive.extractfile("upupa_test/.env.example").read().decode()
        assert "DB_PASSWORD='DA_IMPOSTARE'" in example and read_env(work, "upupa_test", "DB_PASSWORD") not in example
        exported_sql = archive.extractfile("upupa_test/database.sql").read()
        assert b"CREATE TABLE `wp_posts`" in exported_sql

        print("salva come template e crea un sito dal template")
        run_job(http, "POST", f"/api/sites/{site_b}/template", {"name": "upupa_tpl", "title": "Upupa pronta"})
        _, tpls = http.call("GET", "/api/templates", expect=200)
        assert any(t["id"] == "upupa_tpl" and t["snapshot"] for t in tpls), tpls
        run_job(http, "POST", "/api/sites", {"domain": "nuovo.test", "template": "upupa_tpl", "title": "Nuovo"})
        status, html = page("nuovo.test")
        assert status == 200 and "Upupa" in html and "nuovo.test" in html and "template.local" not in html, html[:500]

        print("importa un sito da archivio e dump (prefisso tabelle diverso)")
        buf = io.BytesIO()
        with zipfile.ZipFile(buf, "w") as z:
            for m in archive.getmembers():
                if m.isfile() and m.name.startswith("upupa_test/public/"):
                    rel = m.name[len("upupa_test/public/"):]
                    data = archive.extractfile(m).read()
                    if rel == "wp-config.php":
                        data = b"<?php\n$table_prefix = 'vk_';\n"
                    z.writestr("public_html/" + rel, data)
        upload_import(http, "vecchio.zip", buf.getvalue())
        upload_import(http, "vecchio.sql.gz", gzip.compress(exported_sql.replace(b"`wp_", b"`vk_").replace(b"'wp_", b"'vk_")))
        run_job(http, "POST", "/api/sites/import", {"domain": "importato.test", "title": "Importato", "archive": "vecchio.zip", "dump": "vecchio.sql.gz"})
        assert read_env(work, "importato_test", "TABLE_PREFIX") == "vk_"
        status, html = page("importato.test")
        assert status == 200 and "Upupa" in html and "importato.test" in html, html[:500]
        assert wp_home(compose, work, "importato_test") == "https://importato.test"


        print("restore di un percorso e su nuovo dominio")
        run_job(http, "POST", f"/api/sites/{site_b}/backups")
        _, backups = http.call("GET", f"/api/sites/{site_b}/backups", expect=200)
        snap = backups["runs"][0]["snapshot_id"]
        _, listing = http.call("GET", f"/api/sites/{site_b}/backups/{snap[:12]}/files?path=public/wp-content", expect=200)
        assert any(e["name"] == "themes" and e["is_dir"] for e in listing), listing
        agent(compose, work, {"cmd": "wp", "slug": "upupa_test", "args": ["eval", "file_put_contents(ABSPATH . 'wp-content/themes/index.php', 'rovinato');"]})
        run_job(http, "POST", f"/api/sites/{site_b}/backups/{snap[:12]}/restore", {"mode": "path", "path": "public/wp-content/themes/index.php"})
        restored = sh(*compose, "exec", "-T", "php", "cat", "/srv/sites/upupa_test/public/wp-content/themes/index.php", cwd=work)
        assert "rovinato" not in restored, restored
        verify = run_job(http, "POST", f"/api/sites/{site_b}/backups/{snap[:12]}/restore", {"mode": "new_domain", "domain": "verifica.test"})
        _, vdetail = http.call("GET", f"/api/sites/{verify['site_id']}", expect=200)
        status, html = page("verifica.test", auth=(vdetail["staging"]["user"], vdetail["staging"]["password"]))
        assert status == 200 and "verifica.test" in html, html[:300]

        print("file manager: comprimi ed estrai")
        ops = f"/api/sites/{site_b}/files/ops"
        http.call("POST", ops, {"op": "compress", "paths": ["public/wp-content/themes"], "to": "temi.zip"}, expect=200)
        http.call("POST", ops, {"op": "extract", "path": "temi.zip", "to": "estratti"}, expect=200)
        _, listing = http.call("GET", f"/api/sites/{site_b}/files?path=estratti/themes", expect=200)
        assert listing, "estrazione vuota"
        owner = sh(*compose, "exec", "-T", "php", "stat", "-c", "%U", "/srv/sites/upupa_test/estratti/themes", cwd=work).strip()
        assert owner == "upupa_test", f"proprietario {owner}"

        print("log: ricerca, errori raggruppati")
        page("upupa.test", "/pagina-che-non-esiste/")
        found = wait("richiesta nel log", lambda: http.call("GET", f"/api/sites/{site_b}/logs/access?q=pagina-che-non-esiste", expect=200)[1]["entries"], timeout=30)
        assert found[-1]["uri"] == "/pagina-che-non-esiste/" and found[-1]["ip"] == VISITOR_IP, found
        http.call("GET", f"/api/sites/{site_b}/logs/php-error/groups", expect=200)

        print("override: PHP extra, Caddy del sito, rifiuti")
        http.call("PUT", f"/api/sites/{site_b}/config/php", {"content": "php_value[max_input_vars] = 4321"}, expect=200)
        sh(*compose, "exec", "-T", "php", "grep", "-q", "max_input_vars", "/srv/php/pools/upupa_test.conf", cwd=work)
        status, _ = http.call("PUT", f"/api/sites/{site_b}/config/php", {"content": "php_admin_value[open_basedir] = /"})
        assert status == 400
        http.call("PUT", f"/api/sites/{site_b}/config/caddy", {"content": "header X-Alveare-Prova si"}, expect=200)
        _, _, hdrs = page("upupa.test", headers=True)
        assert hdrs["X-Alveare-Prova"] == "si", dict(hdrs)
        status, body = http.call("PUT", f"/api/sites/{site_b}/config/caddy", {"content": "root * /etc"})
        assert status == 400 and "/etc" in body["error"], body
        status, body = http.call("PUT", f"/api/sites/{site_b}/config/caddy", {"content": "header {"})
        assert status == 400, body
        assert page("upupa.test")[0] == 200, "un override rifiutato non deve rompere il sito"
        http.call("PUT", "/api/config/php", {"content": "max_execution_time = 77"}, expect=200)

        print("blocco IP")
        _, blk = http.call("POST", f"/api/sites/{site_b}/blocks", {"ip": VISITOR_IP, "duration": "1h", "scope": "site"}, expect=201)
        assert page("upupa.test")[0] == 403
        http.call("DELETE", f"/api/blocks/{blk['id']}", expect=200)
        assert page("upupa.test")[0] == 200

        print("cron personalizzati e aggiornamenti WordPress")
        _, c1 = http.call("POST", f"/api/sites/{site_b}/cron", {"expression": "0 3 * * *", "kind": "wp", "command": "option get blogname"}, expect=201)
        run_job(http, "POST", f"/api/sites/{site_b}/cron/{c1['id']}/run")
        _, c2 = http.call("POST", f"/api/sites/{site_b}/cron", {"expression": "0 3 * * *", "kind": "http", "command": "https://upupa.test/"}, expect=201)
        run_job(http, "POST", f"/api/sites/{site_b}/cron/{c2['id']}/run")
        _, wpst = http.call("GET", f"/api/sites/{site_b}/wordpress", expect=200)
        assert re.match(r"\d+\.\d+", wpst["status"]["core"]), wpst
        assert any(p["name"] == "redis-cache" for p in wpst["status"]["plugins"]), wpst
        run_job(http, "POST", f"/api/sites/{site_b}/wordpress/update", {"plugins": {"names": ["redis-cache"]}})
        run_job(http, "POST", f"/api/sites/{site_b}/wp", {"args": ["option", "get", "blogname"]})

        print("PHP 8.4: il sito passa al runtime aggiuntivo e torna")
        http.call("PATCH", f"/api/sites/{site_b}", {"php_version": "8.4"}, expect=200)
        wait("sito su PHP 8.4", lambda: page("upupa.test")[0] == 200, timeout=60)
        version = agent_rt(compose, work, "8.4", {"cmd": "wp", "slug": "upupa_test", "args": ["eval", "echo PHP_VERSION;"]})
        assert version.startswith("8.4"), version
        http.call("PATCH", f"/api/sites/{site_b}", {"php_version": "8.5"}, expect=200)
        wait("sito di nuovo su 8.5", lambda: page("upupa.test")[0] == 200, timeout=60)

        print("worker supervisionati in php-workers")
        lar = run_job(http, "POST", "/api/sites", {"domain": "app.test", "template": "laravel", "title": "App"})
        app_id = lar["site_id"]
        assert "APP_KEY='base64:" in sh(*compose, "exec", "-T", "panel", "cat", "/srv/sites/app_test/.env", cwd=work)
        raw_put(http, f"/api/sites/{app_id}/files/content?path=worker.php", b"<?php while (true) { echo date('c'), \" vivo\\n\"; sleep(1); }")
        http.call("POST", f"/api/sites/{app_id}/workers", {"name": "prova", "command": "php worker.php", "processes": 2, "memory_mb": 64}, expect=201)

        def running():
            _, w = http.call("GET", f"/api/sites/{app_id}/workers", expect=200)
            procs = w["workers"][0]["processes"]
            return len(procs) == 2 and all(p["pid"] for p in procs)

        wait("worker in esecuzione", running, timeout=60)
        wlog = sh(*compose, "exec", "-T", "php-workers", "cat", "/srv/logs/app_test/worker-prova.log", cwd=work)
        assert "vivo" in wlog, wlog
        _, w = http.call("GET", f"/api/sites/{app_id}/workers", expect=200)
        for proc in w["workers"][0]["processes"]:
            user = sh(*compose, "exec", "-T", "php-workers", "stat", "-c", "%U", f"/proc/{proc['pid']}", cwd=work).strip()
            assert user == "app_test", f"worker con l'utente {user!r}"
        _, key = http.call("POST", f"/api/sites/{app_id}/deploy/key", expect=200)
        assert key["public_key"].startswith("ssh-ed25519 "), key

        print("monitoraggio e metriche")
        _, mon = http.call("GET", f"/api/sites/{site_b}/monitor", expect=200)
        req_ = urllib.request.Request(f"http://127.0.0.1:{PORT}/metrics", headers={"Host": PANEL, "Authorization": "Bearer prometheus-prova"})
        with urllib.request.urlopen(req_, timeout=30) as r:
            metrics_text = r.read().decode()
        assert "alveare_open_alerts" in metrics_text and "caddy_http_requests_total" in metrics_text, metrics_text[:500]

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
