#!/usr/bin/env python3
"""本机桌面验收专用的 Chat Completions 模拟服务，不连接外部模型。"""
import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import threading
import time


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--port", type=int, default=18794)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    control = args.output / "control.json"
    if not control.exists():
        control.write_text(json.dumps({"delaySeconds": 0, "status": 200, "message": "fix: 改进会话过期检查"}, ensure_ascii=False))
    lock = threading.Lock()

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_POST(self):
            size = int(self.headers.get("Content-Length", "0"))
            if size > 256 * 1024:
                self.send_error(413)
                return
            data = json.loads(self.rfile.read(size))
            config = json.loads(control.read_text())
            record = {"path": self.path, "hasAuthorization": bool(self.headers.get("Authorization")), "body": data}
            with lock:
                with (args.output / "requests.jsonl").open("a") as log:
                    log.write(json.dumps(record, ensure_ascii=False) + "\n")
            time.sleep(min(float(config["delaySeconds"]), 30))
            response = json.dumps({"choices": [{"message": {"role": "assistant", "content": config["message"]}, "finish_reason": "stop"}]}, ensure_ascii=False).encode()
            try:
                self.send_response(int(config["status"]))
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(response)))
                self.end_headers()
                self.wfile.write(response)
            except (BrokenPipeError, ConnectionResetError):
                pass

    server = ThreadingHTTPServer(("127.0.0.1", args.port), Handler)
    print(f"本机模拟服务：http://127.0.0.1:{server.server_port}/v1", flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        server.server_close()


if __name__ == "__main__":
    main()
