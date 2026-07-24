#!/bin/sh
# Seeds Gitea with a user + demo repo so harvested pages have real content.
# Idempotent: re-runs are no-ops (create commands tolerate already-exists).
# Grafana needs no seeding — provisioning handles it at container start.
set -u
cd "$(dirname "$0")"

docker compose exec -u git gitea gitea admin user create \
    --admin --username verbivore --password verbivore123 \
    --email verbivore@example.com --must-change-password=false \
    2>/dev/null || echo "gitea user exists, skipping"

curl -sf -X POST -u verbivore:verbivore123 \
    http://localhost:42002/api/v1/user/repos \
    -H 'Content-Type: application/json' \
    -d '{"name":"demo","description":"seeded demo repo","auto_init":true}' \
    >/dev/null || echo "gitea repo exists, skipping"

# WordPress: run the installer + a couple of posts through wp-cli.
if docker compose --profile seed run --rm wpcli wp core is-installed 2>/dev/null; then
    echo "wordpress installed, skipping"
else
    docker compose --profile seed run --rm wpcli wp core install \
        --url=http://localhost:42003 --title="Verbivore Test Blog" \
        --admin_user=verbivore --admin_password=verbivore123 \
        --admin_email=verbivore@example.com --skip-email
    docker compose --profile seed run --rm wpcli wp post create \
        --post_title="Digesting the web, one verb at a time" \
        --post_content="A seeded post so harvested pages have real content, links and comment forms." \
        --post_status=publish
    docker compose --profile seed run --rm wpcli wp post create \
        --post_type=page --post_title="About this corpus" \
        --post_content="Seeded page for layout diversity." --post_status=publish
fi

# MediaWiki: sqlite install via the maintenance runner, then a second page.
if docker compose exec mediawiki test -f /var/www/html/LocalSettings.php 2>/dev/null; then
    echo "mediawiki installed, skipping"
else
    docker compose exec mediawiki php maintenance/run.php install \
        --dbtype=sqlite --dbpath=/var/www/data \
        --server=http://localhost:42004 --scriptpath="" \
        --pass=verbivore123 Verbipedia verbivore
    printf 'A seeded article with [[Main Page|a wiki link]] and some text for harvest diversity.' |
        docker compose exec -T mediawiki php maintenance/run.php edit \
            -u verbivore --summary seed "Corpus Article"
fi
# The root-run installer leaves the sqlite file 600/root; apache runs as
# www-data and 500s on it (bit us mid-harvest). Idempotent, so always fix.
docker compose exec mediawiki chown -R www-data:www-data /var/www/data

# Superset: db migrate + admin + examples (the ECharts dashboards ARE the
# point — canvas content the effect model's visual channel exists for).
# load_examples is slow (minutes, pulls data from github) but idempotent.
if docker compose exec superset superset fab list-users 2>/dev/null | grep -q verbivore; then
    echo "superset seeded, skipping"
else
    docker compose exec superset superset db upgrade
    docker compose exec superset superset fab create-admin \
        --username verbivore --firstname Verb --lastname Ivore \
        --email verbivore@example.com --password verbivore123
    docker compose exec superset superset init
    docker compose exec superset superset load-examples || \
        echo "superset examples failed (offline?), dashboards will be sparse"
fi

# Metabase: drive the first-boot wizard through its API (setup token is
# public until setup completes). The bundled sample database provides depth.
if curl -sf http://localhost:42006/api/session/properties | grep -q '"has-user-setup":true'; then
    echo "metabase seeded, skipping"
else
    MB_TOKEN=$(curl -sf http://localhost:42006/api/session/properties |
        sed -n 's/.*"setup-token":"\([^"]*\)".*/\1/p')
    curl -sf -X POST http://localhost:42006/api/setup \
        -H 'Content-Type: application/json' \
        -d "{\"token\":\"$MB_TOKEN\",
             \"user\":{\"email\":\"verbivore@example.com\",\"password\":\"verbivore123!\",
                        \"first_name\":\"Verb\",\"last_name\":\"Ivore\"},
             \"prefs\":{\"site_name\":\"Verbivore Corpus\",\"allow_tracking\":false}}" \
        >/dev/null && echo "metabase setup complete" || echo "metabase setup failed"
fi

# Ghost seeds itself (welcome posts ship with the image); Heimdall has no
# setup at all. Both render harvestable pages from first boot.

# Design-system assets, pinned + reproducible. vendor/ is gitignored; these
# downloads ARE the install step. npm registry tarballs are immutable.
mkdir -p vendor
if [ ! -d vendor/bootstrap-examples ]; then
    curl -sfL -o vendor/bs.zip \
        https://github.com/twbs/bootstrap/releases/download/v5.3.3/bootstrap-5.3.3-examples.zip
    unzip -q vendor/bs.zip -d vendor && mv vendor/bootstrap-5.3.3-examples vendor/bootstrap-examples
    rm vendor/bs.zip
    # The examples index links out to getbootstrap.com; give the crawler a
    # local index instead.
    (cd vendor/bootstrap-examples && ls -d */ | sed 's|/||' |
        awk 'BEGIN{print "<!DOCTYPE html><html><head><title>Bootstrap examples</title></head><body><h1>Examples</h1><ul>"}
             {printf "<li><a href=\"%s/\">%s</a></li>\n", $1, $1}
             END{print "</ul></body></html>"}' > index.html)
fi
if [ ! -d vendor/uswds ]; then
    curl -sfL -o vendor/uswds.tgz https://registry.npmjs.org/@uswds/uswds/-/uswds-3.8.1.tgz
    mkdir -p vendor/uswds-tmp && tar -xzf vendor/uswds.tgz -C vendor/uswds-tmp
    mv vendor/uswds-tmp/package/dist vendor/uswds
    rm -rf vendor/uswds-tmp vendor/uswds.tgz
fi
if [ ! -d vendor/materialize ]; then
    curl -sfL -o vendor/mat.tgz https://registry.npmjs.org/materialize-css/-/materialize-css-1.0.0.tgz
    mkdir -p vendor/mat-tmp && tar -xzf vendor/mat.tgz -C vendor/mat-tmp
    mkdir -p vendor/materialize
    mv vendor/mat-tmp/package/dist/css vendor/materialize/css
    mv vendor/mat-tmp/package/dist/js vendor/materialize/js
    rm -rf vendor/mat-tmp vendor/mat.tgz
fi
if [ ! -d vendor/aria-practices ]; then
    git clone --depth 1 --branch v2024.10.28 https://github.com/w3c/aria-practices vendor/aria-practices 2>/dev/null ||
        git clone --depth 1 https://github.com/w3c/aria-practices vendor/aria-practices
fi
# The repo has no root index; the crawler needs one linking every example.
python3 - <<'PYEOF'
import pathlib
root = pathlib.Path("vendor/aria-practices")
examples = sorted(str(p.relative_to(root)) for p in root.glob("content/patterns/**/*.html"))
rows = "\n".join(f'<li><a href="{e}">{e}</a></li>' for e in examples)
(root / "index.html").write_text(
    f"<!DOCTYPE html><html><head><title>ARIA practices examples</title></head>"
    f"<body><h1>APG examples</h1><ul>{rows}</ul></body></html>")
PYEOF

if [ ! -d vendor/bulma ]; then
    curl -sfL -o vendor/bulma.tgz https://registry.npmjs.org/bulma/-/bulma-1.0.2.tgz
    mkdir -p vendor/bulma-tmp vendor/bulma && tar -xzf vendor/bulma.tgz -C vendor/bulma-tmp
    mv vendor/bulma-tmp/package/css/bulma.min.css vendor/bulma/
    rm -rf vendor/bulma-tmp vendor/bulma.tgz
fi
if [ ! -d vendor/fomantic ]; then
    mkdir -p vendor/fomantic
    curl -sfL -o vendor/fom.tgz https://registry.npmjs.org/fomantic-ui-css/-/fomantic-ui-css-2.9.3.tgz
    mkdir -p vendor/fom-tmp && tar -xzf vendor/fom.tgz -C vendor/fom-tmp
    mv vendor/fom-tmp/package/semantic.min.css vendor/fomantic/
    mv vendor/fom-tmp/package/semantic.min.js vendor/fomantic/
    cp -r vendor/fom-tmp/package/themes vendor/fomantic/themes
    curl -sfL -o vendor/fomantic/jquery.min.js https://registry.npmjs.org/jquery/-/jquery-3.7.1.tgz &&         tar -xzf vendor/fomantic/jquery.min.js -C vendor/fom-tmp 2>/dev/null &&         mv vendor/fom-tmp/package/dist/jquery.min.js vendor/fomantic/jquery.min.js
    rm -rf vendor/fom-tmp vendor/fom.tgz
fi
if [ ! -d vendor/pico ]; then
    mkdir -p vendor/pico
    curl -sfL -o vendor/pico.tgz https://registry.npmjs.org/@picocss/pico/-/pico-2.0.6.tgz
    mkdir -p vendor/pico-tmp && tar -xzf vendor/pico.tgz -C vendor/pico-tmp
    mv vendor/pico-tmp/package/css/pico.min.css vendor/pico/
    rm -rf vendor/pico-tmp vendor/pico.tgz
fi

python3 zengarden-mirror.py vendor/zengarden

# DokuWiki: drive install.php headlessly, then a couple of pages.
if curl -sf http://localhost:42013/doku.php >/dev/null 2>&1 &&
    ! curl -sf http://localhost:42013/install.php | grep -q "Installer" 2>/dev/null; then
    echo "dokuwiki installed, skipping"
else
    curl -sf -X POST http://localhost:42013/install.php \
        -d "l=en&d[title]=Verbiwiki&d[acl]=on&d[superuser]=verbivore&d[fullname]=Verb+Ivore&d[email]=verbivore@example.com&d[password]=verbivore123&d[confirm]=verbivore123&d[policy]=0&d[allowreg]=on&d[license]=cc-by-sa&submit=Save" \
        >/dev/null && echo "dokuwiki installed" || echo "dokuwiki install failed (check manually)"
    curl -sf -X POST "http://localhost:42013/doku.php?id=corpus_article" \
        -d "do=save&id=corpus_article&wikitext=====Corpus article==== A seeded page with a [[start|link home]] and text for harvest diversity.&summary=seed" \
        >/dev/null || true
fi

echo "corpus seeded:"
echo "  grafana   http://localhost:42001"
echo "  gitea     http://localhost:42002"
echo "  wordpress http://localhost:42003"
echo "  mediawiki http://localhost:42004"
echo "  superset  http://localhost:42005"
echo "  metabase  http://localhost:42006"
echo "  ghost     http://localhost:42007"
echo "  heimdall  http://localhost:42008"
echo "  bootstrap http://localhost:42009"
echo "  uswds     http://localhost:42010"
echo "  aria-apg  http://localhost:42011"
echo "  material  http://localhost:42012"
echo "  dokuwiki  http://localhost:42013"
echo "  bulma     http://localhost:42014"
echo "  fomantic  http://localhost:42015"
echo "  pico      http://localhost:42016"
echo "  zengarden http://localhost:42017"
