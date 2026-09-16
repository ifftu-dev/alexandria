#!/usr/bin/env python3
"""Check that the fixture profile answers the way its instructions promise.

Starts `fixture_host`, points the real `alexandria-mcp` binary at the grant it
issues, and calls the tools a person would try first. Every claim the fixture
prints is asserted here, so the banner cannot drift away from what the server
actually returns.

    python3 scripts/mcp/fixture_check.py
"""
import json
import os
import pathlib
import re
import selectors
import signal
import subprocess
import sys
import time

ROOT = pathlib.Path(__file__).resolve().parents[2]
META = {
    "io.modelcontextprotocol/protocolVersion": "2026-07-28",
    "io.modelcontextprotocol/clientInfo": {"name": "fixture-check", "version": "1.0.0"},
    "io.modelcontextprotocol/clientCapabilities": {},
}
failures = []


def check(condition, message):
    if condition:
        print(f"   ok: {message}", flush=True)
    else:
        failures.append(message)
        print(f" FAIL: {message}", flush=True)


class Server:
    """The real stdio binary, spoken to as an independent client."""

    def __init__(self, connection_file):
        env = {**os.environ, "ALEXANDRIA_MCP_CONNECTION_FILE": connection_file}
        self.process = subprocess.Popen(
            [str(ROOT / "target/debug/alexandria-mcp")],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
            text=True, env=env,
        )
        self.selector = selectors.DefaultSelector()
        self.selector.register(self.process.stdout, selectors.EVENT_READ)
        self.next_id = 0

    def call(self, method, params=None):
        self.next_id += 1
        body = dict(params or {})
        body["_meta"] = META
        self.process.stdin.write(
            json.dumps({"jsonrpc": "2.0", "id": self.next_id, "method": method, "params": body}) + "\n"
        )
        self.process.stdin.flush()
        if not self.selector.select(20):
            raise AssertionError(f"{method} timed out")
        return json.loads(self.process.stdout.readline())

    def tool(self, name, arguments=None):
        answer = self.call("tools/call", {"name": name, "arguments": arguments or {}})
        result = answer.get("result", {})
        return result.get("isError", False), result

    def stop(self):
        self.process.terminate()
        try:
            self.process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            self.process.kill()


def main():
    subprocess.run(["cargo", "+1.91.0", "build", "-q", "-p", "alexandria-mcp"], cwd=ROOT, check=True)
    fixture = subprocess.Popen(
        ["cargo", "+1.91.0", "run", "-q", "-p", "alexandria-studio", "--example", "fixture_host"],
        cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
    )
    connection_file, deadline = None, time.time() + 180
    try:
        while time.time() < deadline:
            line = fixture.stdout.readline()
            if not line:
                raise AssertionError("the fixture stopped before it was serving")
            found = re.search(r"connection file\s+(\S+)", line)
            if found:
                connection_file = found.group(1)
            if "Ctrl-C stops it" in line:
                break
        if not connection_file:
            raise AssertionError("the fixture printed no connection file")

        server = Server(connection_file)
        try:
            tools = server.call("tools/list")["result"]["tools"]
            names = sorted(tool["name"] for tool in tools)
            expected = sorted([
                "compute_learning_path", "get_course", "get_credential", "get_learning_progress",
                "get_skill_graph", "list_course_drafts", "list_my_credentials", "propose_lesson_draft",
                "read_lesson", "read_lesson_draft", "resolve_goal", "search_catalog",
                "verify_credential", "verify_presentation",
            ])
            check(names == expected, f"every tool is offered with a grant ({len(names)}: {names})")

            error, result = server.tool("search_catalog", {"query": ""})
            courses = {c["course_id"] for c in result.get("structuredContent", {}).get("items", [])}
            check(not error and courses == {"course-beta", "course-gamma"},
                  f"search_catalog returns the two published courses ({sorted(courses)})")

            error, result = server.tool("get_course", {"course_id": "course-beta"})
            elements = result.get("structuredContent", {}).get("chapters", [{}])[0].get("elements", [])
            kinds = {e["element_id"]: e["content"] for e in elements}
            check(not error and kinds.get("lesson-beta") == "text"
                  and kinds.get("quiz-beta") == "questions"
                  and kinds.get("exam-beta") == "withheld",
                  f"get_course marks the assessment withheld ({kinds})")

            error, result = server.tool(
                "read_lesson", {"course_id": "course-beta", "element_id": "quiz-beta"})
            body = json.dumps(result)
            question = result.get("structuredContent", {}).get("questions", [{}])[0]
            check(not error and question.get("prompt") == "What does beta rest on?"
                  and question.get("options") == ["alpha", "gamma"],
                  "read_lesson returns the quiz question")
            check("ANSWERS LEAKED" not in body and "correct" not in body,
                  "the answer and explanation stay behind")

            error, result = server.tool(
                "read_lesson", {"course_id": "course-beta", "element_id": "exam-beta"})
            check(not error and result.get("structuredContent", {}).get("status") == "withheld"
                  and "ASSESSMENT CONTENT LEAKED" not in json.dumps(result),
                  "the assessment is withheld")

            error, result = server.tool("get_skill_graph")
            nodes = [n["skill_id"] for n in result.get("structuredContent", {}).get("nodes", [])]
            check(not error and nodes == ["skill-alpha"],
                  f"the graph shows only what the owner has been assessed on ({nodes})")

            error, result = server.tool("resolve_goal", {"kind": "exam", "key": "fixture-exam"})
            check(not error and result.get("structuredContent", {}).get("goal_skill_ids") == ["skill-gamma"],
                  "resolve_goal finds the fixture exam")

            error, result = server.tool("compute_learning_path", {"goal_skill_ids": ["skill-gamma"]})
            steps = {s["skill_id"]: s["status"] for s in result.get("structuredContent", {}).get("steps", [])}
            check(not error and steps == {"skill-alpha": "earned", "skill-beta": "available",
                                          "skill-gamma": "locked"},
                  f"the path puts prerequisites first ({steps})")
            recommended = [
                rec["course_id"]
                for step in result.get("structuredContent", {}).get("steps", [])
                for rec in step.get("course_recs", [])
            ]
            check("course-beta" in recommended,
                  f"a course is recommended for the unlocked step ({recommended})")

            error, result = server.tool("list_my_credentials")
            items = result.get("structuredContent", {}).get("items", [])
            check(not error and len(items) == 1 and items[0]["skill_id"] == "skill-alpha",
                  f"one credential summary ({len(items)})")
            check("signed_vc_json" not in json.dumps(result) and "fixture credential" not in json.dumps(result),
                  "the signed document is not part of a summary")

            vector = (ROOT / "crates/alexandria-verify/tests/vectors/01-valid.json").read_text()
            error, result = server.tool(
                "verify_credential", {"credential_json": json.dumps(json.loads(vector)["credential"])})
            verdict = result.get("structuredContent", {})
            check(not error and verdict.get("signature_valid") is True
                  and verdict.get("revocation_status") == "unknown",
                  "verify_credential checks a supplied credential and reports unknown revocation")

            # Drafts: the owner's unpublished course, and only that.
            error, result = server.tool("list_course_drafts")
            listed = [(i["course_id"], i["element_id"]) for i in result.get("structuredContent", {}).get("items", [])]
            check(not error and listed == [("course-draft", "lesson-draft")],
                  f"list_course_drafts shows the owner's draft lesson and nothing else ({listed})")
            error, result = server.tool("search_catalog", {"query": ""})
            check("course-draft" not in json.dumps(result), "the draft is not in the published catalog")
            error, result = server.tool("read_lesson_draft", {"course_id": "course-draft", "element_id": "lesson-draft"})
            draft = result.get("structuredContent", {})
            check(not error and draft.get("text", "").startswith("DRAFT TEXT") and len(draft.get("fingerprint", "")) == 64,
                  "read_lesson_draft returns the owner's text with a fingerprint")
            error, result = server.tool("read_lesson_draft", {"course_id": "course-beta", "element_id": "lesson-beta"})
            check(error and "permission_denied" in json.dumps(result),
                  "another author's lesson is not a draft this person can read")

            proposal = {"course_id": "course-draft", "element_id": "lesson-draft",
                        "fingerprint": draft.get("fingerprint", ""), "request_id": "fixture-check-1",
                        "text": "DRAFT TEXT: proposed by an assistant."}
            error, first = server.tool("propose_lesson_draft", proposal)
            outcome = first.get("structuredContent", {})
            check(not error and outcome.get("status") == "review" and outcome.get("requires_instructor_review") is True,
                  "propose_lesson_draft records a proposal that waits for the owner's review")
            error, again = server.tool("propose_lesson_draft", proposal)
            check(not error and again.get("structuredContent") == first.get("structuredContent"),
                  "an exact retry returns the same outcome rather than a second proposal")
            error, stale = server.tool("propose_lesson_draft", {**proposal, "request_id": "fixture-check-2",
                                                                "fingerprint": "0" * 64})
            check(error and "conflict" in json.dumps(stale),
                  f"a proposal against a stale fingerprint conflicts ({json.dumps(stale)[:200]})")
            error, result = server.tool("read_lesson_draft", {"course_id": "course-draft", "element_id": "lesson-draft"})
            check(result.get("structuredContent", {}).get("text", "").startswith("DRAFT TEXT: the owner"),
                  "a proposal changes nothing until the owner applies it")
        finally:
            server.stop()
    finally:
        fixture.send_signal(signal.SIGINT)
        try:
            fixture.wait(timeout=10)
        except subprocess.TimeoutExpired:
            fixture.kill()

    if failures:
        print(f"\nFAIL: {len(failures)} check(s) failed", file=sys.stderr)
        return 1
    print("\nPASS: the fixture answers the way its instructions promise")
    return 0


if __name__ == "__main__":
    sys.exit(main())
