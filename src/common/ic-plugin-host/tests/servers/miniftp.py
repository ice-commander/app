"""An FTP server just large enough to test copying: LIST, RETR, STOR, passive mode.

    python3 -I miniftp.py <folder> <port>
    MINIFTP_SLOW_KBPS=800  to hold it to a distant server's pace
"""

import os
import socket
import sys
import threading
import time

ROOT = os.path.abspath(sys.argv[1])
PORT = int(sys.argv[2])
SLOW = float(os.environ.get("MINIFTP_SLOW_KBPS", "0"))


def listing(path):
    out = []
    for name in sorted(os.listdir(path)):
        real = os.path.join(path, name)
        st = os.stat(real)
        kind = "d" if os.path.isdir(real) else "-"
        when = time.strftime("%b %d %H:%M", time.localtime(st.st_mtime))
        out.append(f"{kind}rw-r--r-- 1 u u {st.st_size:>12} {when} {name}\r\n")
    return "".join(out).encode()


def serve(conn):
    cwd = ["/"]
    data_sock = [None]
    talking = conn.makefile("rwb")

    def say(text):
        talking.write((text + "\r\n").encode())
        talking.flush()

    def here(path=None):
        path = path if path is not None else cwd[0]
        if not path.startswith("/"):
            path = os.path.join(cwd[0], path)
        return os.path.normpath(os.path.join(ROOT, path.lstrip("/")))

    def accept_data():
        waiting = data_sock[0]
        opened, _ = waiting.accept()
        waiting.close()
        data_sock[0] = None
        return opened

    say("220 mini")
    while True:
        line = talking.readline()
        if not line:
            break
        parts = line.decode(errors="replace").strip().split(" ", 1)
        cmd = parts[0].upper()
        arg = parts[1] if len(parts) > 1 else ""
        if cmd == "USER":
            say("331 need password")
        elif cmd == "PASS":
            say("230 ok")
        elif cmd == "SYST":
            say("215 UNIX Type: L8")
        elif cmd == "TYPE":
            say("200 ok")
        elif cmd == "PWD":
            say(f'257 "{cwd[0]}"')
        elif cmd == "CWD":
            target = os.path.normpath(arg if arg.startswith("/") else os.path.join(cwd[0], arg))
            if os.path.isdir(here(target)):
                cwd[0] = target
                say("250 ok")
            else:
                say("550 no such directory")
        elif cmd == "PASV":
            waiting = socket.socket()
            waiting.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            waiting.bind(("127.0.0.1", 0))
            waiting.listen(1)
            port = waiting.getsockname()[1]
            data_sock[0] = waiting
            say(f"227 Entering Passive Mode (127,0,0,1,{port >> 8},{port & 255})")
        elif cmd == "LIST":
            say("150 here it comes")
            out = accept_data()
            out.sendall(listing(here(arg or None)))
            out.close()
            say("226 done")
        elif cmd == "RETR":
            path = here(arg)
            if not os.path.isfile(path):
                say("550 no such file")
                continue
            say("150 here it comes")
            out = accept_data()
            chunk = 65536
            per = chunk / (SLOW * 1024) if SLOW else 0
            with open(path, "rb") as src:
                while True:
                    piece = src.read(chunk)
                    if not piece:
                        break
                    out.sendall(piece)
                    if per:
                        time.sleep(per)
            out.close()
            say("226 done")
        elif cmd == "STOR":
            path = here(arg)
            os.makedirs(os.path.dirname(path), exist_ok=True)
            say("150 send it")
            incoming = accept_data()
            with open(path, "wb") as dst:
                while True:
                    piece = incoming.recv(65536)
                    if not piece:
                        break
                    dst.write(piece)
            incoming.close()
            say("226 done")
        elif cmd == "SIZE":
            path = here(arg)
            say(f"213 {os.path.getsize(path)}" if os.path.isfile(path) else "550 no")
        elif cmd == "MKD":
            os.makedirs(here(arg), exist_ok=True)
            say("257 made")
        elif cmd == "DELE":
            os.remove(here(arg))
            say("250 gone")
        elif cmd == "RNFR":
            serve.renaming = here(arg)
            say("350 ready")
        elif cmd == "RNTO":
            os.replace(serve.renaming, here(arg))
            say("250 renamed")
        elif cmd == "QUIT":
            say("221 bye")
            break
        else:
            say("502 not implemented")
    try:
        conn.close()
    except OSError:
        pass


listener = socket.socket()
listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
listener.bind(("127.0.0.1", PORT))
listener.listen(8)
print(f"miniftp on {PORT} serving {ROOT}" + (f" at {SLOW} KB/s" if SLOW else ""), flush=True)
while True:
    opened, _ = listener.accept()
    threading.Thread(target=serve, args=(opened,), daemon=True).start()
