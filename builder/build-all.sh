#!/bin/sh

set -e

# gen-version.js rewrites src/common/common/src/version.rs on every build
# (stamping app_type/build_type into each binary). It's a tracked file, so leaving it modified
# dirties the submodule. Revert it to the committed version on exit — on success OR on a failed
# build (set -e) — so the working tree stays clean regardless.
revert_version_rs() {
    git -C src/common checkout -- common/src/version.rs 2>/dev/null || true
}
trap revert_version_rs EXIT

# git reset --hard
# git clean -fd

rm -rf ./distr
mkdir -p ./distr

# Build the web control panel once on the host. The bundle is platform-independent
# JS/CSS that every platform binary embeds via include_bytes!("assets/webui/..."),
# so it must be regenerated BEFORE any cargo build. The docker builders (linux/win)
# and osx.sh pick it up from the mounted source tree.
echo "=== Building web UI (web-app) ==="
npm run build-web-app

./builder/osx.sh
docker compose -f ./builder/ice-commander-builder.yml run --rm exe
docker compose -f ./builder/ice-commander-builder.yml run --rm exe-webserver
docker compose -f ./builder/ice-commander-builder.yml run --rm exe-console
# Each distro builds the GTK app first, then the webserver and the console — both reuse the
# shared CARGO_TARGET_DIR, so only the GTK-free crates relink.
docker compose -f ./builder/ice-commander-builder.yml run --rm zst
docker compose -f ./builder/ice-commander-builder.yml run --rm zst-webserver
docker compose -f ./builder/ice-commander-builder.yml run --rm zst-console
docker compose -f ./builder/ice-commander-builder.yml run --rm deb
docker compose -f ./builder/ice-commander-builder.yml run --rm deb-webserver
docker compose -f ./builder/ice-commander-builder.yml run --rm deb-console
docker compose -f ./builder/ice-commander-builder.yml run --rm rpm
docker compose -f ./builder/ice-commander-builder.yml run --rm rpm-webserver
docker compose -f ./builder/ice-commander-builder.yml run --rm rpm-console



echo ""
echo "=== BUILD COMPLETED ==="
if [ -f "./distr/md5sums.txt" ]; then
    cat ./distr/md5sums.txt
fi
