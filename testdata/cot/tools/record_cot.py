"""Record a Cursor-on-Target corpus from a TAK client, and summarise one that was recorded.

`docs/design/external-standards.md` §5.6 specifies the corpus this produces: what a
client must be made to emit, why each item is needed, and what `SOURCE.md` must record.
This tool is the recording half, so that the procedure is executable rather than
aspirational.

Three transports, two halves of the corpus (`docs/design/tak-interoperability-research.md`
§6 says why there are two):

    python testdata/cot/tools/record_cot.py record --minutes 10
        Join the mesh SA multicast group. A stock ATAK or WinTAK sends TAK Protocol
        Version 1 (protobuf) here from its first datagram, so expect `takproto-v1`.
        Writes `mesh.cotlog` and `mesh.manifest.json`.

    python testdata/cot/tools/record_cot.py record --unicast --minutes 10
        Bind the same UDP port and join nothing, for a client that cannot reach the
        group: an Android emulator sending to its host at 10.0.2.2, or a client with a
        unicast output configured. Same wire form as the group; same two files.

    python testdata/cot/tools/record_cot.py record --tcp --minutes 10
        Listen on a TCP port as a "TAK server" that never speaks. A client stays on
        XML for as long as the server does not advertise version 1, so this yields the
        XML half of the corpus from the same client. Writes `stream.cotlog` and
        `stream.manifest.json`.

    python testdata/cot/tools/record_cot.py summarize

**It records and never authors, and never transmits.** There is no mode that writes CoT
and no code path that sends a byte: a corpus written by the same hand that writes the
decoder passes against itself and fails against every real client (§5.6, "recorded,
never authored"), and a recorder that answered a client's negotiation would have become a
party to it, so the stream half would record the recorder's choices rather than the
client's. If no client is available, the recording does not happen and the codec waits.

Standard library only, like the other tools in this repository.

Output, under `testdata/cot/` unless `--out-dir` says otherwise:

  `<half>.cotlog`         every message, length-delimited, in arrival order: one 24-byte
                          header per record -- magic `COTL`, uint32 payload length,
                          float64 receipt time as a Unix timestamp, uint64 sequence --
                          followed by the bytes as they arrived, verbatim and unparsed.
                          On the stream half a record is one whole message as the
                          client framed it: an XML event through its `</event>`, or a
                          `0xbf`-prefixed protobuf frame through the end of its payload.
  `<half>.manifest.json`  per record: sequence, receipt time, sender address, transport,
                          length, SHA-256, and `wire`, decided by framing bytes alone:
                          `xml`, `takproto-v1` (mesh header), `takproto-v1-stream`
                          (stream header), or a named reason it could not be framed.
                          The manifest never decodes a payload: a recorder that parsed
                          what it recorded would be a second decoder to keep correct, and
                          a wrong one would corrupt the oracle.

The sender address is kept because two clients on one group is item 6 of §5.6's list and
the corpus has to be able to show it. Strip it before publishing if the capture is made
on a network whose addressing is sensitive; `SOURCE.md` records that you did.
"""

import argparse
import hashlib
import json
import os
import selectors
import socket
import struct
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
OUT_DIR = os.path.dirname(HERE)

# ATAK mesh SA default, per PyTAK's configuration documentation (§5.6). The capture is
# what confirms it for the client actually used; this is only the default to try first.
GROUP = "239.2.3.1"
PORT = 6969
# TAK Server's plain TCP streaming port, and the port ATAK offers when a server
# connection is added with SSL off. OpenTAKServer uses 8088 for the same thing.
TCP_PORT = 8087

RECORD_MAGIC = b"COTL"
HEADER = struct.Struct("<4sIdQ")  # magic, payload length, receipt time, sequence
assert HEADER.size == 24

# TAK Protocol Version 1 framing begins with this byte (§5.4). Recognising it is not
# decoding it: the manifest reports which wire form arrived and stops there. On the
# mesh the header is `0xbf <version> 0xbf`; on a stream it is `0xbf <varint length>`.
TAKPROTO_MAGIC = 0xBF
# A stream client delimits XML messages by this token (`takproto/README.txt` in the
# reference client: "Messages are delimited and broken apart by searching for the
# token '</event>'").
EVENT_END = b"</event>"
# A varint longer than this cannot be a length this recorder would ever see.
MAX_VARINT_BYTES = 10

HALVES = ("mesh", "stream")


def paths(out_dir: str, half: str):
    return (
        os.path.join(out_dir, f"{half}.cotlog"),
        os.path.join(out_dir, f"{half}.manifest.json"),
    )


def wire_form_datagram(payload: bytes) -> str:
    return "takproto-v1" if payload[:1] == bytes([TAKPROTO_MAGIC]) else "xml"


def split_stream(buf: bytes):
    """Return `(frame, wire, rest)` for the first whole message in `buf`, or None.

    Framing only. A `0xbf` at a message boundary is followed by a varint payload length
    and that many bytes; anything else is XML and ends at the first `</event>`. The
    frame is returned verbatim, header included, because the decoder's fixture test
    should meet the framing exactly as the client sent it.
    """
    if not buf:
        return None
    if buf[0] == TAKPROTO_MAGIC:
        length = 0
        shift = 0
        i = 1
        while True:
            if i >= len(buf):
                if i > MAX_VARINT_BYTES:
                    return bytes(buf), "malformed-varint", b""
                return None
            byte = buf[i]
            length |= (byte & 0x7F) << shift
            i += 1
            if not byte & 0x80:
                break
            shift += 7
            if shift > 63:
                return bytes(buf), "malformed-varint", b""
        end = i + length
        if len(buf) < end:
            return None
        return bytes(buf[:end]), "takproto-v1-stream", bytes(buf[end:])
    idx = buf.find(EVENT_END)
    if idx < 0:
        return None
    end = idx + len(EVENT_END)
    return bytes(buf[:end]), "xml", bytes(buf[end:])


class Corpus:
    """One half of the corpus: the log it appends to and the manifest it keeps."""

    def __init__(self, out_dir: str, half: str, append: bool):
        self.log_path, self.manifest_path = paths(out_dir, half)
        self.entries = []
        if os.path.exists(self.log_path) and not append:
            print(f"refusing to overwrite {self.log_path}: move it aside, or pass --append", file=sys.stderr)
            raise SystemExit(2)
        if append and os.path.exists(self.manifest_path):
            with open(self.manifest_path, encoding="utf-8") as fh:
                self.entries = json.load(fh)["datagrams"]
        self.seq = self.entries[-1]["sequence"] + 1 if self.entries else 0
        self.started = time.time()
        self.out = open(self.log_path, "ab" if append else "wb")

    def write(self, payload: bytes, sender: str, wire: str, transport: str) -> None:
        now = time.time()
        self.out.write(HEADER.pack(RECORD_MAGIC, len(payload), now, self.seq))
        self.out.write(payload)
        self.out.flush()
        self.entries.append({
            "sequence": self.seq,
            "receipt_time": round(now, 6),
            "sender": sender,
            "transport": transport,
            "length": len(payload),
            "sha256": hashlib.sha256(payload).hexdigest(),
            "wire": wire,
        })
        self.seq += 1
        if self.seq % 10 == 0:
            print(f"  {self.seq} records", end="\r", flush=True)

    def close(self) -> None:
        self.out.close()
        doc = {
            "note": "Recorded by testdata/cot/tools/record_cot.py. Payloads are never decoded here.",
            "recorded_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(self.started)),
            "datagrams": self.entries,
        }
        with open(self.manifest_path, "w", encoding="utf-8") as fh:
            json.dump(doc, fh, indent=1)
            fh.write("\n")


def open_udp(port: int, group, iface: str) -> socket.socket:
    """Bind `port`; join `group` on `iface` unless `group` is None (unicast)."""
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM, socket.IPPROTO_UDP)
    s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    if hasattr(socket, "SO_REUSEPORT"):  # not on Windows; harmless where it exists
        try:
            s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEPORT, 1)
        except OSError:
            pass
    # Bind to the port on every interface. Binding to the group address works on Linux
    # and not on Windows, and the corpus is recorded on whichever host has the client.
    s.bind(("", port))
    if group is not None:
        mreq = socket.inet_aton(group) + socket.inet_aton(iface)
        s.setsockopt(socket.IPPROTO_IP, socket.IP_ADD_MEMBERSHIP, mreq)
    s.setblocking(False)
    return s


def open_tcp_listener(port: int) -> socket.socket:
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    s.bind(("", port))
    s.listen(8)
    s.setblocking(False)
    return s


def record(args) -> int:
    if args.tcp is not None:
        half, transport = "stream", "tcp"
    elif args.unicast:
        half, transport = "mesh", "unicast-udp"
    else:
        half, transport = "mesh", "multicast"
    corpus = Corpus(args.out_dir, half, args.append)
    sel = selectors.DefaultSelector()
    streams = {}  # connected socket -> (peer, bytearray)

    if transport == "tcp":
        listener = open_tcp_listener(args.tcp)
        sel.register(listener, selectors.EVENT_READ, "listen")
        print(f"listening on tcp port {args.tcp}; this recorder never sends, so the client stays on XML")
    else:
        group = None if args.unicast else args.group
        udp = open_udp(args.port, group, args.interface)
        sel.register(udp, selectors.EVENT_READ, "udp")
        where = f"udp port {args.port}, no group" if args.unicast else f"{args.group}:{args.port} via {args.interface}"
        print(f"listening on {where}")
    print(f"for {args.minutes} min: walk the six items in external-standards.md section 5.6, then Ctrl-C or wait out")

    deadline = time.time() + args.minutes * 60

    def close_stream(conn):
        peer, buf = streams.pop(conn)
        sel.unregister(conn)
        conn.close()
        if buf:
            # Bytes the client sent and never completed. Kept under a reason, never
            # dropped, so the corpus can say what happened at the end of a connection.
            corpus.write(bytes(buf), peer, "partial-at-close", transport)

    try:
        while time.time() < deadline:
            for key, _ in sel.select(timeout=1.0):
                if key.data == "udp":
                    payload, addr = key.fileobj.recvfrom(65535)
                    corpus.write(payload, f"{addr[0]}:{addr[1]}", wire_form_datagram(payload), transport)
                elif key.data == "listen":
                    conn, addr = key.fileobj.accept()
                    conn.setblocking(False)
                    streams[conn] = (f"{addr[0]}:{addr[1]}", bytearray())
                    sel.register(conn, selectors.EVENT_READ, "stream")
                    print(f"  connection from {addr[0]}:{addr[1]}")
                else:
                    conn = key.fileobj
                    try:
                        chunk = conn.recv(65535)
                    except OSError:
                        chunk = b""
                    if not chunk:
                        close_stream(conn)
                        continue
                    peer, buf = streams[conn]
                    buf += chunk
                    while True:
                        split = split_stream(buf)
                        if split is None:
                            break
                        frame, wire, rest = split
                        corpus.write(frame, peer, wire, transport)
                        buf[:] = rest
    except KeyboardInterrupt:
        print("\nstopped")
    finally:
        for conn in list(streams):
            close_stream(conn)
        for key in list(sel.get_map().values()):
            key.fileobj.close()
        sel.close()
        corpus.close()

    return summarize(args)


def read_log(log_path: str):
    """Yield (receipt time, sequence, payload) for every record, checking the framing."""
    with open(log_path, "rb") as fh:
        while True:
            head = fh.read(HEADER.size)
            if not head:
                return
            if len(head) < HEADER.size:
                raise ValueError(f"truncated header at offset {fh.tell() - len(head)}")
            magic, length, receipt, seq = HEADER.unpack(head)
            if magic != RECORD_MAGIC:
                raise ValueError(f"bad record magic {magic!r} at offset {fh.tell() - HEADER.size}")
            payload = fh.read(length)
            if len(payload) < length:
                raise ValueError(f"truncated payload for sequence {seq}")
            yield receipt, seq, payload


def summarize_half(out_dir: str, half: str) -> bool:
    """Print the numbers SOURCE.md has to carry for one half, read back from the files."""
    log_path, manifest_path = paths(out_dir, half)
    if not os.path.exists(log_path):
        return False
    count = 0
    total = 0
    first = last = None
    digest = hashlib.sha256()
    with open(log_path, "rb") as fh:
        digest.update(fh.read())
    for receipt, _seq, payload in read_log(log_path):
        count += 1
        total += len(payload)
        first = receipt if first is None else first
        last = receipt
    print(f"half            {half}")
    print(f"file            {log_path}")
    print(f"sha256          {digest.hexdigest()}")
    print(f"records         {count}")
    print(f"payload bytes   {total}")
    if first is not None:
        print(f"span            {last - first:.1f} s")
    if os.path.exists(manifest_path):
        by_wire, senders, transports = {}, {}, {}
        with open(manifest_path, encoding="utf-8") as fh:
            for e in json.load(fh)["datagrams"]:
                by_wire[e["wire"]] = by_wire.get(e["wire"], 0) + 1
                senders[e["sender"]] = senders.get(e["sender"], 0) + 1
                t = e.get("transport", "unrecorded")
                transports[t] = transports.get(t, 0) + 1
        print(f"wire forms      {by_wire}")
        print(f"transports      {transports}")
        print(f"senders         {senders}")
        if len(senders) < 2:
            print("  note: one sender only. Item 6 of section 5.6 (a second client) is uncovered on this half.")
    print()
    return True


def summarize(args) -> int:
    found = [summarize_half(args.out_dir, half) for half in HALVES]
    if not any(found):
        print(f"no corpus: neither half exists under {args.out_dir}", file=sys.stderr)
        return 2
    if not all(found):
        missing = [h for h, f in zip(HALVES, found) if not f]
        print(f"  note: no {missing[0]} half. The corpus has two halves (tak-interoperability-research.md section 6).")
    return 0


def main(argv) -> int:
    p = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    p.add_argument("--out-dir", default=OUT_DIR, help="where the halves are written and read (default: testdata/cot)")
    sub = p.add_subparsers(dest="cmd", required=True)

    r = sub.add_parser("record", help="listen and record until the clock runs out")
    r.add_argument("--minutes", type=float, default=10.0)
    r.add_argument("--group", default=GROUP)
    r.add_argument("--port", type=int, default=PORT, help="UDP port for the mesh half")
    r.add_argument("--interface", default="0.0.0.0", help="local interface address to join the group on")
    r.add_argument("--unicast", action="store_true", help="bind the UDP port and join no group")
    r.add_argument("--tcp", nargs="?", const=TCP_PORT, type=int, metavar="PORT",
                   help=f"listen on a TCP port as a server that never speaks (default port {TCP_PORT})")
    r.add_argument("--append", action="store_true", help="add to an existing half")
    r.set_defaults(func=record)

    s = sub.add_parser("summarize", help="re-read a recorded corpus and print SOURCE.md's numbers")
    s.set_defaults(func=summarize)

    args = p.parse_args(argv)
    if getattr(args, "tcp", None) is not None and getattr(args, "unicast", False):
        p.error("--tcp and --unicast are different halves; run them separately")
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
