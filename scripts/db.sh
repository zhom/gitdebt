#!/usr/bin/env bash
# Bring up the local gitdebt Postgres + Redis in Docker.
# Usage:
#   scripts/db.sh up      # default — start (or recreate) the containers, wait until healthy
#   scripts/db.sh down    # stop the containers, keep the postgres volume
#   scripts/db.sh psql    # open a psql shell against the running db
#   scripts/db.sh redis   # open a redis-cli shell against the running redis
#   scripts/db.sh logs    # tail postgres + redis logs
#
# Wiping the volume is intentionally NOT exposed here. If you really
# need to drop the data, do it deliberately:
#   docker compose down && docker volume rm gitdebt_postgres-data
set -euo pipefail
cd "$(dirname "$0")/.."

cmd="${1:-up}"

wait_healthy() {
  local container="$1"
  echo -n "waiting for ${container} to be healthy"
  for _ in $(seq 1 30); do
    status=$(docker inspect -f '{{.State.Health.Status}}' "$container" 2>/dev/null || echo "starting")
    if [[ "$status" == "healthy" ]]; then
      echo " ok"
      return 0
    fi
    echo -n "."
    sleep 1
  done
  echo " timed out"
  docker compose logs --tail=50
  return 1
}

case "$cmd" in
  up)
    docker compose up -d postgres redis
    wait_healthy gitdebt-postgres
    wait_healthy gitdebt-redis
    echo
    echo "postgres up at: postgres://gitdebt:gitdebt@localhost:5432/gitdebt"
    echo "redis up at:    redis://localhost:6390"
    echo "set DATABASE_URL / REDIS_URL accordingly (see backend/.env.example)"
    ;;
  down)
    docker compose down
    ;;
  psql)
    docker exec -it gitdebt-postgres psql -U gitdebt -d gitdebt
    ;;
  redis)
    docker exec -it gitdebt-redis redis-cli
    ;;
  logs)
    docker compose logs -f postgres redis
    ;;
  *)
    echo "usage: $0 {up|down|psql|redis|logs}" >&2
    exit 2
    ;;
esac
