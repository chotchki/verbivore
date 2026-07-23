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

echo "corpus seeded: grafana http://localhost:42001 gitea http://localhost:42002"
