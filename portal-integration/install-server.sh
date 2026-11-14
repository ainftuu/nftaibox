#!/usr/bin/env bash
# Adds only toolbox authorization to the existing NFTAI source snapshot.
# It deliberately does not run apply-theme.sh, change billing, or clone latest.
set -euo pipefail
umask 022
[ "$(id -u)" = 0 ] || { echo 'Use sudo bash install-server.sh'; exit 1; }
RELEASE="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
(cd "$RELEASE" && sha256sum -c SHA256SUMS)
DEPLOY=/opt/nftai/deploy
SOURCE="$DEPLOY/sub2api-theme/.src"
STAMP="$(date +%Y%m%d-%H%M%S)-$$"
BACKUP="/opt/nftai/backups/toolbox-$STAMP"
STAGE="$DEPLOY/toolbox-builds/$STAMP"
IMAGE="localhost/nftai-sub2api:toolbox-$STAMP"
LOG="/tmp/nftai-toolbox-$STAMP.log"
SWITCHED=0
exec > >(tee -a "$LOG") 2>&1
cd "$DEPLOY"
test -f .env && test -f docker-compose.yml && test -f "$SOURCE/Dockerfile"
OLD_IMAGE="$(docker inspect --format '{{.Image}}' sub2api)"
OLD_CURRENCY="$(docker inspect --format '{{ index .Config.Labels "fun.nftai.ledger-currency" }}' sub2api)"
echo "Source version: $(git -C "$SOURCE" describe --tags --always)"
echo "Retaining billing currency: $OLD_CURRENCY"
echo "Log: $LOG"
mkdir -p "$BACKUP" "$(dirname "$STAGE")"
chmod 700 "$BACKUP"
cp -p .env "$BACKUP/deploy.env"
cp -p docker-compose.yml "$BACKUP/docker-compose.yml"
cp -p sub2api-theme/apply-theme.sh "$BACKUP/apply-theme.sh"
cp -p Caddyfile "$BACKUP/Caddyfile"
docker image tag "$OLD_IMAGE" "localhost/nftai-sub2api:before-toolbox-$STAMP"
docker image save "localhost/nftai-sub2api:before-toolbox-$STAMP" | gzip > "$BACKUP/image.tar.gz"
docker compose exec -T postgres pg_dump -U sub2api -d sub2api -Fc > "$BACKUP/database.dump"
docker compose exec -T postgres pg_restore --list < "$BACKUP/database.dump" > "$BACKUP/database.contents"
test -s "$BACKUP/database.contents"
tar -czf "$BACKUP/source.tar.gz" --exclude=.git --exclude=node_modules -C "$SOURCE" .
(cd "$BACKUP" && sha256sum image.tar.gz database.dump database.contents source.tar.gz > SHA256SUMS && sha256sum -c SHA256SUMS)
cat > "$BACKUP/rollback.sh" <<EOF
#!/usr/bin/env bash
set -euo pipefail
cd '$DEPLOY'
cp -p '$BACKUP/deploy.env' .env
cat '$BACKUP/Caddyfile' > Caddyfile
docker compose exec -T caddy caddy reload --config /etc/caddy/Caddyfile
docker compose up -d --no-deps --pull never sub2api
echo 'Previous image restored. No database restore was performed.'
EOF
chmod 700 "$BACKUP/rollback.sh"
echo "Backup verified: $BACKUP"
echo "Rollback: sudo bash $BACKUP/rollback.sh"
trap 'code=$?; if [ "$SWITCHED" = 1 ]; then echo "Deployment failed; restoring previous image."; bash "$BACKUP/rollback.sh" || true; fi; echo "Failure code $code. Log: $LOG"; exit "$code"' ERR

mkdir -p "$STAGE"
cp -a "$SOURCE/." "$STAGE/"
docker run --rm -v "$RELEASE":/patch:ro -v "$STAGE":/source node:24-alpine node /patch/apply.cjs /source
# Retain the previously fixed entrypoint permission when rebuilding this snapshot.
sed -i 's@RUN chmod +x /app/docker-entrypoint.sh@RUN chmod 0755 /app/docker-entrypoint.sh@' "$STAGE/Dockerfile"
grep -q 'RUN chmod 0755 /app/docker-entrypoint.sh' "$STAGE/Dockerfile"
python3 - "$STAGE/Dockerfile" <<'PY'
from pathlib import Path
import sys
p=Path(sys.argv[1]); text=p.read_text()
anchor='# Build the binary (BuildType=release for CI builds, embed frontend)'
if anchor not in text: raise SystemExit('Unsupported Docker build layout')
gate='\n'.join([
    '# NFTAI toolbox authorization regression gate',
    'RUN --mount=type=cache,id=sub2api-gomod,target=/go/pkg/mod '+chr(92),
    '    --mount=type=cache,id=sub2api-gobuild,target=/root/.cache/go-build '+chr(92),
    "    GOMAXPROCS=2 go test -p 2 ./internal/handler -run '^TestToolboxCapabilities$' -count=1",
    '', '',
])
p.write_text(text.replace(anchor,gate+anchor))
PY
DOCKER_BUILDKIT=1 docker build --pull=false \
  --build-arg GOPROXY=https://proxy.golang.org,direct \
  --build-arg GOSUMDB=sum.golang.org \
  --build-arg VERSION="$(git -C "$SOURCE" describe --tags --always | sed 's/^v//')" \
  --label fun.nftai.toolbox-protocol=1 \
  --label fun.nftai.entrypoint-permissions=0755 \
  -t "$IMAGE" "$STAGE"
NEW_CURRENCY="$(docker image inspect --format '{{ index .Config.Labels "fun.nftai.ledger-currency" }}' "$IMAGE")"
[ "$NEW_CURRENCY" = "$OLD_CURRENCY" ] || { echo 'Billing currency changed unexpectedly; refusing deployment'; exit 1; }
docker run --rm --network none --entrypoint /app/docker-entrypoint.sh "$IMAGE" /bin/true

if grep -q '^SUB2API_IMAGE=' .env; then
  sed -i "s#^SUB2API_IMAGE=.*#SUB2API_IMAGE=$IMAGE#" .env
else
  printf '\nSUB2API_IMAGE=%s\n' "$IMAGE" >> .env
fi
SWITCHED=1
docker run --rm -v "$RELEASE":/patch:ro -v "$DEPLOY":/deploy node:24-alpine node /patch/apply-caddy.cjs /deploy/Caddyfile
docker compose exec -T caddy caddy validate --config /etc/caddy/Caddyfile
docker compose exec -T caddy caddy reload --config /etc/caddy/Caddyfile
docker compose up -d --no-deps --pull never sub2api
HEALTHY=0
for attempt in $(seq 1 40); do
  if docker compose exec -T sub2api wget -q -T 3 -O /dev/null http://localhost:8080/health 2>/dev/null; then HEALTHY=1; break; fi
  sleep 3
done
[ "$HEALTHY" = 1 ]
CAPS="$(docker compose exec -T sub2api wget -q -O - http://localhost:8080/api/v1/auth/toolbox-capabilities)"
printf '%s' "$CAPS" | python3 -c 'import json,sys; v=json.load(sys.stdin); assert v["code"]==0 and v["data"]["protocol"]==1'
PAGE="$(docker compose exec -T sub2api wget -q -O - http://localhost:8080/toolbox/authorize)"
test -n "$PAGE"
echo "Authorization capability verified: $CAPS"
echo "Active image: $IMAGE"
mkdir -p "$DEPLOY/sub2api-theme/portal-integration"
cp -a "$RELEASE/." "$DEPLOY/sub2api-theme/portal-integration/"
docker run --rm -v "$DEPLOY/sub2api-theme":/theme node:24-alpine node /theme/portal-integration/persist-theme-hook.cjs /theme/apply-theme.sh
printf '%s\n' "$STAGE" > "$DEPLOY/ACTIVE-TOOLBOX-SOURCE"
SWITCHED=0
trap - ERR
echo "TOOLBOX_AUTH_DEPLOYED; retained currency=$NEW_CURRENCY; backup=$BACKUP; log=$LOG"
