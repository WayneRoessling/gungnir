"""Record a Cursor-on-Target mesh corpus, and summarise one that was recorded.

`docs/design/external-standards.md` §5.6 specifies the corpus this produces: what a
client must be made to emit, why each item is needed, and what `SOURCE.md` must record.
This tool is the recording half, so that the procedure is executable rather than
aspirational.

    python testdata/cot/tools/record_cot.py record --minutes 10
    python testdata/cot/tools/record_cot.py summarize

**It records and never authors.** There is no mode that writes CoT: a corpus written by
the same hand that writes the decoder passes against itself and fails against every real
client, which is the outcome GAP-064's rule exists to prevent (§5.6, "recorded, never
authored"). If no client is available, the recording does not happen and the codec waits.

Standard library only, like the other tools in this repository.

Output, both under `testdata/cot/`:

  `mesh.cotlog`       every datagram, length-delimited, in arrival order:
                      one 24-byte header per record -- magic `COTL`, uint32 payload
                      length, float64 receipt time as a Unix timestamp, uint64 sequence
                      -- followed by the payload bytes, verbatim and unparsed.
  `mesh.manifest.json`  per datagram: sequence, receipt time, sender address, length,
                      SHA-256, and `wire`, which is `xml` or `takproto-v1` decided by the
                      0xbf framing byte alone. The manifest never decodes a payload: a
                      recorder that parsed what it recorded would be a second decoder to
                      keep correct, and a wrong one would corrupt the oracle.

The sender address is kept because two clients on one group is item 6 of §5.6's list and
the corpus has to be able to show it. Strip it before publishing if the capture is made
on a network whose addressing is sensitive; `SOURCE.md` records that you did.
"""

import argparse
import hashlib
import json
import os
import socket
import struct
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
OUT_DIR = os.path.dirname(HERE)
LOG = os.path.join(OUT_DIR, "mesh.cotlog")
MANIFEST = os.path.join(OUT_DIR, "mesh.manifest.json")

# ATAK mesh SA default, per PyTAK's configuration documentation (§5.6). The capture is
# what confirms it for the client actually used; this is only the default to try first.
GROUP = "239.2.3.1"
PORT = 6969

RECORD_MAGIC = b"COTL"
HEADER = struct.Struct("<4sIdQ")  # magic, payload length, receipt time, sequence
assert HEADER.size == 24

# TAK Protocol Version 1 framing begins with this byte (§5.4). Recognising it is not
# decoding it: the manifest reports which wire form arrived and stops there.
TAKPROTO_MAGIC = 0xBF


def open_socket(group: str, port: int, iface: str) -> socket.socket:
    """Join `group` on `iface` and return a socket bound to `port`."""
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
    mreq = socket.inet_aton(group) + socket.inet_aton(iface)
    s.setsockopt(socket.IPPROTO_IP, socket.IP_ADD_MEMBERSHIP, mreq)
    s.settimeout(1.0)
    return s


def wire_form(payload: bytes) -> str:
    return "takproto-v1" if payload[:1] == bytes([TAKPROTO_MAGIC]) else "xml"


def record(args) -> int:
    if os.path.exists(LOG) and not args.append:
        print(f"refusing to overwrite {LOG}: move it aside, or pass --append", file=sys.stderr)
        return 2
    deadline = time.time() + args.minutes * 60
    entries = []
    if args.append and os.path.exists(MANIFEST):
        with open(MANIFEST, encoding="utf-8") as fh:
            entries = json.load(fh)["datagrams"]
    seq = entries[-1]["sequence"] + 1 if entries else 0

    sock = open_socket(args.group, args.port, args.interface)
    print(f"listening on {args.group}:{args.port} via {args.interface} for {args.minutes} min")
    print("walk the six items in external-standards.md section 5.6, then Ctrl-C or wait out")
    started = time.time()
    try:
        with open(LOG, "ab" if args.append else "wb") as out:
            while time.time() < deadline:
                try:
                    payload, addr = sock.recvfrom(65535)
                except socket.timeout:
                    continue
                except KeyboardInterrupt:
                    break
                now = time.time()
                out.write(HEADER.pack(RECORD_MAGIC, len(payload), now, seq))
                out.write(payload)
                out.flush()
                entries.append({
                    "sequence": seq,
                    "receipt_time": round(now, 6),
                    "sender": f"{addr[0]}:{addr[1]}",
                    "length": len(payload),
                    "sha256": hashlib.sha256(payload).hexdigest(),
                    "wire": wire_form(payload),
                })
                seq += 1
                if seq % 10 == 0:
                    print(f"  {seq} datagrams", end="\r", flush=True)
    except KeyboardInterrupt:
        print("\nstopped")
    finally:
        sock.close()

    write_manifest(entries, started)
    summarize(args)
    return 0


def write_manifest(entries, started) -> None:
    doc = {
        "note": "Recorded by testdata/cot/tools/record_cot.py. Payloads are never decoded here.",
        "recorded_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(started)),
        "datagrams": entries,
    }
    with open(MANIFEST, "w", encoding="utf-8") as fh:
        json.dump(doc, fh, indent=1)
        fh.write("\n")


def read_log():
    """Yield (header fields, payload) for every record, checking the framing."""
    with open(LOG, "rb") as fh:
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


def summarize(args) -> int:
    """Print the numbers SOURCE.md has to carry, read back from the files."""
    if not os.path.exists(LOG):
        print(f"no corpus: {LOG} does not exist", file=sys.stderr)
        return 2
    count = 0
    by_wire = {}
    first = last = None
    total = 0
    digest = hashlib.sha256()
    with open(LOG, "rb") as fh:
        digest.update(fh.read())
    for receipt, seq, payload in read_log():
        count += 1
        total += len(payload)
        by_wire[wire_form(payload)] = by_wire.get(wire_form(payload), 0) + 1
        first = receipt if first is None else first
        last = receipt
    print(f"file            {LOG}")
    print(f"sha256          {digest.hexdigest()}")
    print(f"datagrams       {count}")
    print(f"payload bytes   {total}")
    print(f"wire forms      {by_wire}")
    if first is not None:
        print(f"span            {last - first:.1f} s")
    senders = {}
    if os.path.exists(MANIFEST):
        with open(MANIFEST, encoding="utf-8") as fh:
            for e in json.load(fh)["datagrams"]:
                senders[e["sender"]] = senders.get(e["sender"], 0) + 1
        print(f"senders         {senders}")
        if len(senders) < 2:
            print("  note: one sender only. Item 6 of section 5.6 (a second client) is uncovered.")
    return 0


def main(argv) -> int:
    p = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    sub = p.add_subparsers(dest="cmd", required=True)

    r = sub.add_parser("record", help="join the group and record until the clock runs out")
    r.add_argument("--minutes", type=float, default=10.0)
    r.add_argument("--group", default=GROUP)
    r.add_argument("--port", type=int, default=PORT)
    r.add_argument("--interface", default="0.0.0.0", help="local interface address to join on")
    r.add_argument("--append", action="store_true", help="add to an existing corpus")
    r.set_defaults(func=record)

    s = sub.add_parser("summarize", help="re-read a recorded corpus and print SOURCE.md's numbers")
    s.set_defaults(func=summarize)

    args = p.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
