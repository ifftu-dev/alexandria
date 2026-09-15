#!/usr/bin/env python3
import json
import pathlib
import selectors
import subprocess
import sys

root = pathlib.Path(__file__).resolve().parents[2]
binary = pathlib.Path(sys.argv[1]) if len(sys.argv) > 1 else root / 'target/debug/alexandria-mcp'
meta = {
    'io.modelcontextprotocol/protocolVersion': '2026-07-28',
    'io.modelcontextprotocol/clientInfo': {'name': 'independent-python-smoke', 'version': '1.0.0'},
    'io.modelcontextprotocol/clientCapabilities': {},
}
process = subprocess.Popen([str(binary)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
selector = selectors.DefaultSelector()
selector.register(process.stdout, selectors.EVENT_READ)

def call(request_id, method, arguments=None, include_meta=True):
    params = dict(arguments or {})
    if include_meta:
        params['_meta'] = meta
    process.stdin.write(json.dumps({'jsonrpc': '2.0', 'id': request_id, 'method': method, 'params': params}) + '\n')
    process.stdin.flush()
    if not selector.select(5):
        raise AssertionError('MCP response timed out')
    response = json.loads(process.stdout.readline())
    assert response.get('id') == request_id, response
    return response

try:
    discovery = call(1, 'server/discover')
    assert 'result' in discovery, discovery
    listing = call(2, 'tools/list')
    tools = listing['result']['tools']
    assert len(tools) == 1 and tools[0]['name'] == 'verify_credential'
    assert 'outputSchema' in tools[0]
    for index, (fixture, valid) in enumerate([('01-valid.json', True), ('02-tampered-payload.json', False)], 3):
        data = json.loads((root / 'crates/alexandria-verify/tests/vectors' / fixture).read_text())
        response = call(index, 'tools/call', {'name': 'verify_credential', 'arguments': {'credential_json': json.dumps(data['credential'])}})
        result = response['result']['structuredContent']
        assert result['signature_valid'] == valid
        assert result['revocation_status'] == 'unknown'
    rejected = call(5, 'tools/list', include_meta=False)
    assert 'error' in rejected, rejected
    unknown = call(6, 'tools/call', {'name': 'publish_course', 'arguments': {}})
    assert 'error' in unknown or unknown.get('result', {}).get('isError'), unknown
    print('PASS: independent stdio discovery, schemas, valid/tampered verification, missing metadata, and unavailable write tools')
finally:
    process.stdin.close()
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait()
    selector.close()
