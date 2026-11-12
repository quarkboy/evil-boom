#!/usr/bin/env python3
"""openai-relay.py [PORT]: a local relay to OpenAI's API for Gary (docs/GARY.md).

Gary speaks OpenAI's Chat Completions API; OpenAI's newer models want the Responses API for
much of what Gary asks (function tools with a reasoning effort; `max_completion_tokens` for
`max_tokens`; strict schemas with every property required). This takes Gary's chat completion
requests, sends them to /v1/responses, and gives the answers back as chat completions: messages
as input items (a tool call and its result as a function call and its output), tools and the
choice of them, a structured output's schema as the text format, the reasoning effort, the
output limit. Schemas go not strict: Gary's actions have optional fields, which strict mode
refuses, and Gary checks what comes back against them anyway.

Gary's OpenAI engine points at http://127.0.0.1:PORT/v1 (8090 unless given). The request's
Authorization header (the key, in Gary's settings) is passed on as it is and never logged or
kept. Only requests to the local address are taken; anything but a chat completion is passed
through as it is.
"""
import json
import sys
import time
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

UPSTREAM = "https://api.openai.com"
PASSED = ("Authorization", "Content-Type", "OpenAI-Organization", "OpenAI-Project")


def text_of(content):
    """A chat message's content as plain text (a string, or a list of text parts)."""
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        return "\n".join(p.get("text", "") for p in content if isinstance(p, dict))
    return ""


def to_responses(chat):
    """A chat completion request as a Responses API one."""
    instructions, items = [], []
    for m in chat.get("messages", []):
        role = m.get("role")
        if role in ("system", "developer"):
            instructions.append(text_of(m.get("content")))
        elif role == "tool":
            items.append({"type": "function_call_output", "call_id": m.get("tool_call_id", ""),
                          "output": text_of(m.get("content"))})
        else:
            text = text_of(m.get("content"))
            if text:
                items.append({"role": role, "content": text})
            for call in m.get("tool_calls") or []:
                f = call.get("function", {})
                items.append({"type": "function_call", "call_id": call.get("id", ""),
                              "name": f.get("name", ""), "arguments": f.get("arguments", "{}")})
    req = {"model": chat["model"], "input": items, "store": False}
    if instructions:
        req["instructions"] = "\n\n".join(instructions)
    if chat.get("tools"):
        req["tools"] = [{"type": "function", "name": t["function"]["name"],
                         "description": t["function"].get("description", ""),
                         "parameters": t["function"].get("parameters") or {"type": "object", "properties": {}},
                         "strict": False}
                        for t in chat["tools"] if t.get("type") == "function"]
    choice = chat.get("tool_choice")
    if isinstance(choice, dict) and choice.get("type") == "function":
        req["tool_choice"] = {"type": "function", "name": choice["function"]["name"]}
    elif isinstance(choice, str):
        req["tool_choice"] = choice
    fmt = chat.get("response_format") or {}
    if fmt.get("type") == "json_schema":
        js = fmt.get("json_schema", {})
        req["text"] = {"format": {"type": "json_schema", "name": js.get("name", "answer"),
                                  "schema": js.get("schema", {}), "strict": False}}
    elif fmt.get("type") == "json_object":
        req["text"] = {"format": {"type": "json_object"}}
    if chat.get("reasoning_effort"):
        req["reasoning"] = {"effort": chat["reasoning_effort"]}
    limit = chat.get("max_completion_tokens") or chat.get("max_tokens")
    if limit:
        req["max_output_tokens"] = limit
    return req


def to_chat(resp, model):
    """A Responses API answer as a chat completion."""
    text, calls, reasoning = [], [], []
    for item in resp.get("output", []):
        kind = item.get("type")
        if kind == "message":
            text += [p.get("text", "") for p in item.get("content", [])
                     if p.get("type") == "output_text"]
        elif kind == "function_call":
            calls.append({"id": item.get("call_id", ""), "type": "function",
                          "function": {"name": item.get("name", ""),
                                       "arguments": item.get("arguments", "{}")}})
        elif kind == "reasoning":
            reasoning += [s.get("text", "") for s in item.get("summary", [])]
    message = {"role": "assistant", "content": "".join(text) or None}
    if calls:
        message["tool_calls"] = calls
    if reasoning:
        message["reasoning"] = "\n".join(reasoning)
    usage = resp.get("usage") or {}
    return {
        "id": resp.get("id", ""), "object": "chat.completion",
        "created": int(resp.get("created_at") or time.time()), "model": resp.get("model", model),
        "choices": [{"index": 0, "message": message,
                     "finish_reason": "tool_calls" if calls else "stop"}],
        "usage": {"prompt_tokens": usage.get("input_tokens", 0),
                  "completion_tokens": usage.get("output_tokens", 0),
                  "total_tokens": usage.get("total_tokens", 0)},
    }


class Relay(BaseHTTPRequestHandler):
    def _send(self, path, body):
        headers = {h: self.headers[h] for h in PASSED if self.headers.get(h)}
        if body is not None:
            headers["Content-Type"] = "application/json"
        req = urllib.request.Request(UPSTREAM + path, data=body, headers=headers,
                                     method=self.command)
        try:
            with urllib.request.urlopen(req, timeout=120) as r:
                return r.status, r.read(), r.headers.get("Content-Type", "")
        except urllib.error.HTTPError as e:
            return e.code, e.read(), e.headers.get("Content-Type", "")
        except urllib.error.URLError as e:
            err = json.dumps({"error": {"message": str(e.reason)}}).encode()
            return 502, err, "application/json"

    def _answer(self, status, data, ctype):
        self.send_response(status)
        self.send_header("Content-Type", ctype or "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        status, data, ctype = self._send(self.path, None)
        self._answer(status, data, ctype)
        # (what passed, never the headers: the key's in them)
        sys.stderr.write(f"GET {self.path} -> {status}\n")

    def do_POST(self):
        body = self.rfile.read(int(self.headers.get("Content-Length") or 0))
        if not self.path.rstrip("/").endswith("/chat/completions"):
            status, data, ctype = self._send(self.path, body)
            self._answer(status, data, ctype)
            sys.stderr.write(f"POST {self.path} -> {status}\n")
            return
        try:
            chat = json.loads(body)
        except ValueError:
            self._answer(400, b'{"error":{"message":"not JSON"}}', "application/json")
            return
        status, data, ctype = self._send("/v1/responses", json.dumps(to_responses(chat)).encode())
        if status == 200:
            data = json.dumps(to_chat(json.loads(data), chat.get("model", ""))).encode()
            ctype = "application/json"
        self._answer(status, data, ctype)
        sys.stderr.write(f"POST {self.path} (as /v1/responses) -> {status}\n")

    def log_message(self, *_):
        pass  # (said in the handlers instead: what passed, never the headers)


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8090
    print(f"relaying http://127.0.0.1:{port} -> {UPSTREAM} (chat completions as responses)",
          flush=True)
    ThreadingHTTPServer(("127.0.0.1", port), Relay).serve_forever()
