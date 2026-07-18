#!/usr/bin/env bash
# Bring up the local gitdebt Postgres in Docker.
# Usage:
#   scripts/db.sh up      # default — start (or recreate) the container, wait until healthy
#   scripts/db.sh down    # stop the container, keep the volume
#   scripts/db.sh psql    # open a psql shell against the running db
#   scripts/db.sh logs    # tail postgres logs
#
# Wiping the volume is intentionally NOT exposed here. If you really
# need to drop the data, do it deliberately:
#   docker compose down && docker volume rm gitdebt_postgres-data
set -euo pipefail
cd "$(dirname "$0")/.."

cmd="${1:-up}"

case "$cmd" in
  up)
    docker compose up -d postgres
    echo -n "waiting for postgres to be healthy"
    for i in $(seq 1 30); do
      status=$(docker inspect -f '{{.State.Health.Status}}' gitdebt-postgres 2>/dev/null || echo "starting")
      if [[ "$status" == "healthy" ]]; then
        echo " ok"
        echo
        echo "postgres up at: postgres://gitdebt:gitdebt@localhost:5432/gitdebt"
        echo "set DATABASE_URL accordingly (see backend/.env.example)"
        exit 0
      fi
      echo -n "."
      sleep 1
    done
    echo " timed out"
    docker compose logs --tail=50 postgres
    exit 1
    ;;
  down)
    docker compose down
    ;;
  psql)
    docker exec -it gitdebt-postgres psql -U gitdebt -d gitdebt
    ;;
  logs)
    docker compose logs -f postgres
    ;;
  *)
    echo "usage: $0 {up|down|psql|logs}" >&2
    exit 2
    ;;
esac
