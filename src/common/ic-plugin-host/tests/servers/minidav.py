"""A WebDAV server just large enough to test copying: PROPFIND, GET, PUT, MKCOL, DELETE, COPY, MOVE.

    python3 -I minidav.py <folder> <port>
    MINIDAV_SLOW_KBPS=800  to hold it to a distant server's pace
"""

import html
import os
import socketserver
import sys
import time
import urllib.parse
from http.server import BaseHTTPRequestHandler

ROOT = os.path.abspath(sys.argv[1])
PORT = int(sys.argv[2])
SLOW = float(os.environ.get("MINIDAV_SLOW_KBPS", "0"))


def local(path):
    return os.path.normpath(os.path.join(ROOT, urllib.parse.unquote(path.split("?")[0]).lstrip("/")))


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *a):
        pass

    def nothing(self, code):
        self.send_response(code)
        self.send_header("Content-Length", "0")
        self.end_headers()

    def do_OPTIONS(self):
        self.send_response(200)
        self.send_header("DAV", "1,2")
        self.send_header("Allow", "OPTIONS,GET,PUT,DELETE,PROPFIND,MKCOL,COPY,MOVE")
        self.send_header("Content-Length", "0")
        self.end_headers()

    def do_PROPFIND(self):
        asked = int(self.headers.get("Content-Length") or 0)
        if asked:
            self.rfile.read(asked)
        target = local(self.path)
        if not os.path.exists(target):
            return self.nothing(404)
        items = [(self.path, target)]
        if os.path.isdir(target) and self.headers.get("Depth", "1") != "0":
            for name in sorted(os.listdir(target)):
                items.append((self.path.rstrip("/") + "/" + urllib.parse.quote(name),
                              os.path.join(target, name)))
        out = ['<?xml version="1.0" encoding="utf-8"?><D:multistatus xmlns:D="DAV:">']
        for href, real in items:
            st = os.stat(real)
            when = time.strftime("%a, %d %b %Y %H:%M:%S GMT", time.gmtime(st.st_mtime))
            out.append(f"<D:response><D:href>{html.escape(href)}</D:href><D:propstat><D:prop>")
            out.append(f"<D:displayname>{html.escape(os.path.basename(real))}</D:displayname>")
            out.append("<D:resourcetype><D:collection/></D:resourcetype>" if os.path.isdir(real)
                       else f"<D:resourcetype/><D:getcontentlength>{st.st_size}</D:getcontentlength>")
            out.append(f"<D:getlastmodified>{when}</D:getlastmodified>")
            out.append("</D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response>")
        out.append("</D:multistatus>")
        body = "".join(out).encode()
        self.send_response(207)
        self.send_header("Content-Type", 'application/xml; charset="utf-8"')
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        target = local(self.path)
        if not os.path.isfile(target):
            return self.nothing(404)
        self.send_response(200)
        self.send_header("Content-Type", "application/octet-stream")
        self.send_header("Content-Length", str(os.path.getsize(target)))
        self.end_headers()
        chunk = 65536
        per = chunk / (SLOW * 1024) if SLOW else 0
        with open(target, "rb") as f:
            while True:
                piece = f.read(chunk)
                if not piece:
                    break
                self.wfile.write(piece)
                if per:
                    time.sleep(per)

    def do_PUT(self):
        target = local(self.path)
        os.makedirs(os.path.dirname(target), exist_ok=True)
        said = self.headers.get("Content-Length")
        with open(target, "wb") as f:
            if said is not None:
                left = int(said)
                while left > 0:
                    piece = self.rfile.read(min(65536, left))
                    if not piece:
                        break
                    f.write(piece)
                    left -= len(piece)
            else:
                while True:
                    line = self.rfile.readline().strip()
                    if not line:
                        break
                    length = int(line, 16)
                    if length == 0:
                        self.rfile.readline()
                        break
                    f.write(self.rfile.read(length))
                    self.rfile.readline()
        self.nothing(201)

    def do_COPY(self):
        source = local(self.path)
        target = local(urllib.parse.urlparse(self.headers.get("Destination", "")).path)
        if not os.path.isfile(source):
            return self.nothing(404)
        os.makedirs(os.path.dirname(target), exist_ok=True)
        with open(source, "rb") as a, open(target, "wb") as b:
            while True:
                piece = a.read(1 << 20)
                if not piece:
                    break
                b.write(piece)
        self.nothing(201)

    def do_MOVE(self):
        target = local(urllib.parse.urlparse(self.headers.get("Destination", "")).path)
        os.makedirs(os.path.dirname(target), exist_ok=True)
        os.replace(local(self.path), target)
        self.nothing(201)

    def do_MKCOL(self):
        os.makedirs(local(self.path), exist_ok=True)
        self.nothing(201)

    def do_DELETE(self):
        target = local(self.path)
        if os.path.isdir(target):
            os.rmdir(target)
        elif os.path.exists(target):
            os.remove(target)
        self.nothing(204)


class Threaded(socketserver.ThreadingTCPServer):
    allow_reuse_address = True
    daemon_threads = True


print(f"minidav on {PORT} serving {ROOT}" + (f" at {SLOW} KB/s" if SLOW else ""), flush=True)
Threaded(("127.0.0.1", PORT), Handler).serve_forever()
