# Copyright (C) 2026 Roessling Digital Solutions LLC
# SPDX-License-Identifier: AGPL-3.0-or-later
# Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

"""The SAPIENT loopback fixture US-09 needs (`docs/ux/usability-round-1-session.md`
group D): a minimal, generic "always accept" SAPIENT sensor, standing in for the real
one neither the desktop nor the node baseline is wired to.

Run it, note the address it prints, and point a `sapient_feeds` entry at it in whichever
baseline the session uses -- `round-1.json` for the desktop, or, for a node-backed
setup, the node baseline the moderator writes from it during US-04's dry run (it is not
a committed file; see `SOURCE.md`). Neither carries such an entry today, so adding it is
a session-setup step this script does not do for you:

    "sapient_feeds": [{
        "name": "loopback",
        "sensor_id": <a sensor already in the baseline's sensor list>,
        "node_type": "spotter",
        "source": {"kind": "tcp", "addr": "127.0.0.1:PORT"},
        "destination_id": "<any UUID-shaped string>"
    }]

and `sapient_node_id` set to any non-empty string, so `SensorControl::issue` has a
destination and a node identity to task from.

Run inside .venv-oracles, or any Python 3 with no extra packages:

    ../../../.venv-oracles/Scripts/python.exe sapient_loopback.py [--port N]

WHAT IT DOES, exactly, and nothing more: reads newline-delimited JSON objects (the same
framing `gungnir_ingest::adapters::sapient::TcpSapientSource::take_messages` reads and
`TcpTaskSink::send` writes -- see that module's own doc comment on why JSON over TCP
rather than the binary wire format); for any message carrying `task.taskId`, writes back
one line naming that same id `TASK_STATUS_ACCEPTED`, unconditionally, on the same
connection. It does not simulate a sensor's actual behaviour, does not track cardinality,
and does not decide whether a command is sensible for the sensor type: this is a wire
protocol responder for exercising a live session's acknowledgement path, not a sensor
model.

WHY A LOOPBACK, NOT A SCRIPT OF TIMED RESPONSES: `gungnir-app/tests/sapient_task_ack.rs`
and `gungnir-node/src/main.rs`'s own inline tests already gate the desktop's and the
node's own halves of this exchange against a real socket; what neither proves is a
person watching the desktop UI see a command answered on the wire in real time, during
an actual moderated session, which is what US-09 needs. Accepting immediately and always
is the simplest fixture that makes "commits" (an acknowledgement, not just an issued
command) observable; a fixture that sometimes refused or delayed would be a different,
scripted scenario nobody has asked for yet.

THE CONNECTION MUST STAY OPEN for the session's duration. A first draft of the Rust-side
proof of this same protocol (`gungnir-node/src/main.rs::
a_task_ack_written_back_on_the_wire_is_read_by_the_real_adapter_and_applied`) closed the
socket right after writing the ack and found that `TcpSapientSource::take_messages`
treats reaching end-of-file as "the middleware closed the connection" -- correctly, for
a genuine mid-session disconnect -- discarding whatever it had just buffered in the same
read rather than returning it. This script accepts once and keeps serving tasks on that
same connection for as long as the client holds it open, rather than closing after one
exchange, for exactly that reason.
"""

import argparse
import json
import socketserver


class LoopbackHandler(socketserver.StreamRequestHandler):
    def handle(self):
        peer = f"{self.client_address[0]}:{self.client_address[1]}"
        print(f"[sapient-loopback] connection from {peer}", flush=True)
        for raw in self.rfile:
            line = raw.decode("utf-8", errors="replace").strip()
            if not line:
                continue
            try:
                message = json.loads(line)
            except json.JSONDecodeError as exc:
                print(f"[sapient-loopback] {peer}: not JSON, ignored ({exc})", flush=True)
                continue
            task = message.get("task")
            if not isinstance(task, dict) or "taskId" not in task:
                kind = next(iter(message.keys() - {"timestamp", "nodeId"}), "unknown")
                print(f"[sapient-loopback] {peer}: received a {kind!r} message, not a task; ignored", flush=True)
                continue
            task_id = task["taskId"]
            command = task.get("command", {})
            print(f"[sapient-loopback] {peer}: task {task_id} ({command}) -> accepted", flush=True)
            ack = {
                "nodeId": "sapient-loopback-fixture",
                "taskAck": {"taskId": task_id, "taskStatus": "TASK_STATUS_ACCEPTED"},
            }
            self.wfile.write((json.dumps(ack) + "\n").encode("utf-8"))
        print(f"[sapient-loopback] {peer}: connection closed", flush=True)


class ReusableServer(socketserver.ThreadingTCPServer):
    allow_reuse_address = True
    daemon_threads = True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--host", default="127.0.0.1", help="address to listen on (default: 127.0.0.1)"
    )
    parser.add_argument(
        "--port",
        type=int,
        default=0,
        help="port to listen on (default: 0, let the OS choose and print it)",
    )
    args = parser.parse_args()

    with ReusableServer((args.host, args.port), LoopbackHandler) as server:
        host, port = server.server_address
        print(f"[sapient-loopback] listening on {host}:{port}", flush=True)
        print("[sapient-loopback] point a sapient_feeds entry's \"source\" at this address", flush=True)
        print("[sapient-loopback] Ctrl-C to stop", flush=True)
        try:
            server.serve_forever()
        except KeyboardInterrupt:
            print("\n[sapient-loopback] stopped", flush=True)


if __name__ == "__main__":
    main()
