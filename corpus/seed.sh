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

echo "corpus seeded:"
echo "  grafana   http://localhost:42001"
echo "  gitea     http://localhost:42002"
echo "  wordpress http://localhost:42003"
echo "  mediawiki http://localhost:42004"
echo "  superset  http://localhost:42005"
echo "  metabase  http://localhost:42006"
echo "  ghost     http://localhost:42007"
echo "  heimdall  http://localhost:42008"
