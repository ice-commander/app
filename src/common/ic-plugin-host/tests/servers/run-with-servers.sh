#!/bin/bash
# Runs the cross-plugin copy tests against SFTP, FTP and WebDAV servers started here and torn
# down afterwards. Nothing is installed and nothing outside the scratch folder is touched.
#
#   tests/servers/run-with-servers.sh [cargo test arguments]
#
# The SFTP half needs sshd and sftp-server on this machine; without them that part is skipped.
set -u

HERE="$(cd "$(dirname "$0")" && pwd)"
APP="$(cd "$HERE/../../../../.." && pwd)"
WORK="$(mktemp -d)"
trap 'kill $(jobs -p) 2>/dev/null; rm -rf "$WORK"' EXIT

free_port() {
    python3 -c "import socket;s=socket.socket();s.bind(('127.0.0.1',0));print(s.getsockname()[1]);s.close()"
}

DAV_PORT=$(free_port)
FTP_PORT=$(free_port)
SFTP_PORT=$(free_port)

mkdir -p "$WORK/dav" "$WORK/ftp" "$WORK/sftp" "$WORK/landing"

MINIDAV_SLOW_KBPS="${IC_SLOW_KBPS:-0}" python3 -I "$HERE/minidav.py" "$WORK/dav" "$DAV_PORT" >"$WORK/dav.log" 2>&1 &
MINIFTP_SLOW_KBPS="${IC_SLOW_KBPS:-0}" python3 -I "$HERE/miniftp.py" "$WORK/ftp" "$FTP_PORT" >"$WORK/ftp.log" 2>&1 &

SFTP_READY=no
if [ -x /usr/sbin/sshd ] && [ -x /usr/lib/openssh/sftp-server ]; then
    ssh-keygen -q -t ed25519 -f "$WORK/hostkey" -N '' 2>/dev/null
    ssh-keygen -q -t ed25519 -f "$WORK/userkey" -N '' 2>/dev/null
    cat > "$WORK/sshd_config" <<EOF
Port $SFTP_PORT
ListenAddress 127.0.0.1
HostKey $WORK/hostkey
PidFile $WORK/sshd.pid
AuthorizedKeysFile $WORK/userkey.pub
StrictModes no
UsePAM no
PasswordAuthentication no
PubkeyAuthentication yes
Subsystem sftp /usr/lib/openssh/sftp-server
EOF
    chmod 600 "$WORK/hostkey" "$WORK/userkey"
    if /usr/sbin/sshd -f "$WORK/sshd_config" -E "$WORK/sshd.log"; then
        SFTP_READY=yes
    fi
fi

# Waits for each port rather than sleeping a guessed amount.
for port in "$DAV_PORT" "$FTP_PORT"; do
    python3 - "$port" <<'PY'
import socket, sys, time
port = int(sys.argv[1])
for _ in range(100):
    try:
        socket.create_connection(("127.0.0.1", port), 0.2).close()
        sys.exit(0)
    except OSError:
        time.sleep(0.05)
sys.exit(f"nothing answering on {port}")
PY
done

export IC_DAV_URL="http://127.0.0.1:$DAV_PORT"
export IC_DAV_ROOT="$WORK/dav"
export IC_FTP_ADDR="127.0.0.1:$FTP_PORT"
export IC_FTP_ROOT="$WORK/ftp"
export IC_LANDING="$WORK/landing"
export IC_SLOW_KBPS="${IC_SLOW_KBPS:-0}"
if [ "$SFTP_READY" = yes ]; then
    export IC_SFTP_ADDR="127.0.0.1:$SFTP_PORT"
    export IC_SFTP_USER="${USER:-$(id -un)}"
    export IC_SFTP_KEY="$WORK/userkey"
    export IC_SFTP_ROOT="$WORK/sftp"
else
    echo "no sshd here: the SFTP half is skipped" >&2
fi

echo "dav $DAV_PORT · ftp $FTP_PORT · sftp ${SFTP_READY/no/skipped}"
cd "$APP"
cargo test -p ic-plugin-host --test copying_between_plugins -- --ignored --nocapture "$@"
