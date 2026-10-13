"""MUX capture adapter. No raw flow dump or credentials are persisted."""
import asyncio
import codecs
import ctypes
import json
import html
import ipaddress
import os
import re
import subprocess
import time
import tempfile
import urllib.request
import zlib
from mitmproxy import dns
from pathlib import Path
from urllib.parse import urlsplit, urlunsplit, parse_qsl, urlencode, quote
from mitmproxy import ctx
from mitmproxy.proxy import mode_servers

ROOT = Path(os.environ['MUX_CAPTURE_DIR'])
CONFIG = json.loads((ROOT / 'session.json').read_text())
LIMIT = 2 * 1024 * 1024
MAX_FLOWS = 5000
SENSITIVE = re.compile(r'authorization|cookie|password|passwd|secret|token|api[-_]?key|credential|session[-_]?key|^key$', re.I)
PUBLIC_USAGE_FIELDS = {'max_tokens', 'max_output_tokens', 'max_completion_tokens', 'budget_tokens', 'reasoning_tokens', 'audio_tokens', 'input_tokens', 'output_tokens', 'prompt_tokens', 'completion_tokens', 'total_tokens', 'cached_tokens', 'cache_read_input_tokens', 'cache_creation_input_tokens', 'input_tokens_details', 'output_tokens_details', 'prompt_tokens_details', 'completion_tokens_details', 'prompttokencount', 'candidatestokencount', 'thoughtstokencount', 'totaltokencount', 'cachedcontenttokencount', 'tooluseprompttokencount'}


def sensitive(key):
    return key.lower() not in PUBLIC_USAGE_FIELDS and SENSITIVE.search(key)


def private_json(path, data):
    fd, temporary = tempfile.mkstemp(prefix='.' + path.stem + '-', suffix='.tmp', dir=path.parent)
    try:
        with os.fdopen(fd, 'w') as out:
            json.dump(data, out, ensure_ascii=False)
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def redact(data):
    if isinstance(data, dict):
        return {key: '[REDACTED]' if sensitive(key) else redact(value) for key, value in data.items()}
    if isinstance(data, list):
        return [redact(value) for value in data]
    return data


def decoded_body(msg):
    raw = msg.raw_content or b''
    encoding = msg.headers.get('content-encoding', '').strip().lower()
    if encoding in ('gzip', 'deflate'):
        decoder = zlib.decompressobj(16 + zlib.MAX_WBITS if encoding == 'gzip' else zlib.MAX_WBITS)
        return decoder.decompress(raw, LIMIT + 1)
    if encoding in ('', 'identity'):
        return raw[:LIMIT + 1]
    raise ValueError('Unsupported content encoding')


def credentials(flow, extra=None):
    values = set()
    def add(value):
        if isinstance(value, str) and len(value) >= 4 and len(value) <= 4096 and len(values) < 128:
            values.add(value)
    def walk(value):
        if isinstance(value, dict):
            for key, child in value.items():
                if sensitive(key):
                    add(child)
                else:
                    walk(child)
        elif isinstance(value, list):
            for child in value:
                walk(child)
    for direction in ('request', 'response'):
        msg = getattr(flow, direction)
        if msg is None:
            continue
        for key, value in msg.headers.items(multi=True):
            if SENSITIVE.search(key):
                add(value)
                if key.lower() in ('authorization', 'proxy-authorization'):
                    add(value.split(' ', 1)[-1])
                if 'cookie' in key.lower():
                    for item in (value.split(';')[:1] if key.lower() == 'set-cookie' else value.split(';')):
                        if '=' in item:
                            add(item.split('=', 1)[1].strip())
        try:
            stream = msg.stream if isinstance(msg.stream, BodyStream) else None
            if stream and (stream.omitted or stream.failed or not stream.complete):
                continue
            body = stream.body if stream else decoded_body(msg)
            if len(body) <= LIMIT:
                walk(json.loads(body))
        except (ValueError, TypeError, RecursionError, zlib.error):
            pass
    for key, value in parse_qsl(urlsplit(flow.request.pretty_url).query):
        if SENSITIVE.search(key):
            add(value)
    address = urlsplit(flow.request.pretty_url)
    add(address.username)
    add(address.password)
    walk(extra)
    return sorted(values, key=len, reverse=True)


def clean_text(text, secrets=()):
    for secret in secrets:
        for form in {secret, quote(secret, safe=''), html.escape(secret), json.dumps(secret, ensure_ascii=False)[1:-1]}:
            text = text.replace(form, '[REDACTED]')
    # Also mask common credential formats outside structured JSON.
    text = re.sub(r'(?i)(Bearer\s+)[A-Za-z0-9._~+/-]+', r'\1[REDACTED]', text)
    return re.sub(r'\b(?:sk-[A-Za-z0-9_-]{16,}|AIza[A-Za-z0-9_-]{20,}|ya29\.[A-Za-z0-9._-]+)\b', '[REDACTED]', text)


def redact_event(block, secrets):
    # SSE joins multiple data lines before parsing their JSON payload.
    rows = block.splitlines()
    data = '\n'.join(line[5:].lstrip(' ') for line in rows if line.startswith('data:'))
    if data:
        try:
            data = json.dumps(redact(json.loads(data)), ensure_ascii=False)
        except (ValueError, RecursionError):
            if data.lstrip().startswith(('{', '[')) and data.strip() != '[DONE]':
                data = '[非标准 JSON 事件；未保存内容]'
            else:
                data = re.sub(r'(?i)((?:password|secret|token|api[-_]?key)[\"\']?\s*[=:]\s*)[^\s&<]+', r'\1[REDACTED]', data)
        first = True
        result = []
        for line in rows:
            if line.startswith('data:'):
                if first:
                    result.append('data: ' + data)
                    first = False
            else:
                result.append(line)
        rows = result
    return clean_text('\n'.join(rows), secrets) + '\n\n'


def clean_url(url):
    value = urlsplit(url)
    # User-info is authentication, not part of the request navigator address.
    host = value.hostname or ''
    if ':' in host:
        host = '[' + host + ']'
    authority = host + (':' + str(value.port) if value.port is not None else '')
    return urlunsplit((value.scheme, authority, value.path, urlencode([(key, '[REDACTED]' if SENSITIVE.search(key) else val) for key, val in parse_qsl(value.query, keep_blank_values=True)]), ''))


def message(msg, secrets, stream=None):
    if msg is None:
        return None
    if stream and stream.wire_bytes and not stream.complete:
        # A partial document may contain a credential whose defining field has
        # not arrived yet. Never persist it before learning the complete body.
        return {'headers': [[key, '[REDACTED]' if SENSITIVE.search(key) else clean_text(value, secrets)] for key, value in msg.headers.items(multi=True)],
                'body': '[正文尚未完整接收；未保存内容]', 'bytes': stream.wire_bytes, 'truncated': True, 'encoding': 'text'}
    raw = msg.raw_content or b''
    try:
        if stream and stream.failed:
            raise ValueError('Unsafe streamed body')
        body = stream.body if stream else decoded_body(msg)
    except (ValueError, zlib.error):
        return {'headers': [[key, '[REDACTED]' if SENSITIVE.search(key) else clean_text(value, secrets)] for key, value in msg.headers.items(multi=True)], 'body': '[无法安全解压正文；未保存正文]', 'bytes': stream.wire_bytes if stream else len(raw), 'truncated': True, 'encoding': 'text'}
    if len(body) > LIMIT or stream and stream.omitted:
        # Do not partially persist structured credentials from a truncated JSON.
        text = '[正文超过 2 MiB；未保存正文]'
        encoding = 'text'
    else:
        try:
            text = body.decode('utf-8')
            try:
                text = json.dumps(redact(json.loads(text)), ensure_ascii=False, indent=2)
            except (ValueError, TypeError, RecursionError):
                # SSE may carry structured credentials in individual events.
                if msg.headers.get('content-type', '').startswith('text/event-stream'):
                    lines = []
                    for line in text.splitlines():
                        if line.startswith('data:'):
                            try:
                                line = 'data: ' + json.dumps(redact(json.loads(line[5:].strip())), ensure_ascii=False)
                            except ValueError:
                                pass
                        lines.append(line)
                    text = '\n'.join(lines)
                elif msg.headers.get('content-type', '').lower().find('json') >= 0 or text.lstrip().startswith(('{', '[')):
                    text = '[无法安全解析 JSON 正文；未保存正文]'
                elif msg.headers.get('content-type', '').lower().startswith('application/x-www-form-urlencoded'):
                    text = urlencode([(key, '[REDACTED]' if sensitive(key) else value) for key, value in parse_qsl(text, keep_blank_values=True)])
                elif msg.headers.get('content-type', '').lower().startswith('multipart/'):
                    text = '[Multipart 正文；未保存正文]'
                else:
                    text = re.sub(r'(?i)((?:password|secret|token|api[-_]?key)\s*[=:]\s*)[^\s&<]+', r'\1[REDACTED]', text)
            text = clean_text(text, secrets)
            if len(text.encode('utf-8')) > LIMIT:
                text = '[脱敏正文超过 2 MiB；未保存正文]'
            encoding = 'text'
        except UnicodeDecodeError:
            # Binary bodies may contain secrets and cannot be safely redacted.
            text = '[二进制正文；未保存正文]'
            encoding = 'binary'
    headers = [[key, '[REDACTED]' if SENSITIVE.search(key) else clean_text(value, secrets)] for key, value in msg.headers.items(multi=True)]
    return {'headers': headers, 'body': text, 'bytes': stream.wire_bytes if stream else len(raw),
            'truncated': len(body) > LIMIT or bool(stream and stream.omitted), 'encoding': encoding}


class BodyStream:
    """Forward every wire byte; retain at most LIMIT decoded bytes for recording."""
    def __init__(self, msg, omitted=False):
        self.body = bytearray()
        self.wire_bytes = 0
        self.omitted = omitted
        self.complete = False
        encoding = msg.headers.get('content-encoding', '').strip().lower()
        self.failed = encoding not in ('', 'identity', 'gzip', 'deflate')
        self.decoder = None
        if not omitted and not self.failed:
            if encoding == 'gzip':
                self.decoder = zlib.decompressobj(16 + zlib.MAX_WBITS)
            elif encoding == 'deflate':
                self.decoder = zlib.decompressobj()

    def __call__(self, chunk):
        self.wire_bytes += len(chunk)
        self.complete = not chunk
        if not self.omitted and not self.failed:
            try:
                decoded = self.decoder.decompress(chunk, LIMIT - len(self.body) + 1) if self.decoder else chunk
                if len(self.body) + len(decoded) > LIMIT:
                    self.omitted = True
                elif not chunk and self.decoder and not self.decoder.eof:
                    self.failed = True
                else:
                    self.body.extend(decoded)
            except zlib.error:
                self.failed = True
            if self.omitted or self.failed:
                self.body.clear()
                self.decoder = None
        return chunk


def stream_body(flow, direction):
    """Known small bodies stay native; unknown lengths must also stay bounded."""
    msg = getattr(flow, direction)
    try:
        length = int(msg.headers.get('content-length', ''))
    except ValueError:
        length = None
    if length is not None and 0 <= length <= LIMIT:
        return
    stream = BodyStream(msg, omitted=length is not None and length > LIMIT)
    msg.stream = stream


def captured_message(flow, direction, secrets):
    msg = getattr(flow, direction)
    stream = msg.stream if msg is not None and isinstance(msg.stream, BodyStream) else None
    return message(msg, secrets, stream)


LIBPROC = ctypes.CDLL('/usr/lib/libproc.dylib')


def process_path(pid, fallback):
    buffer = ctypes.create_string_buffer(4096)
    if LIBPROC.proc_pidpath(pid, buffer, len(buffer)) > 0:
        return os.path.realpath(os.fsdecode(buffer.value))
    return fallback


def processes():
    result = subprocess.run(['/bin/ps', '-axo', 'pid=,ppid=,lstart=,comm='], capture_output=True, text=True, timeout=5, check=True)
    rows = []
    for line in result.stdout.splitlines():
        fields = line.strip().split(None, 7)
        if len(fields) == 8 and fields[0].isdigit() and fields[1].isdigit():
            pid = int(fields[0])
            rows.append((pid, int(fields[1]), process_path(pid, fields[7]), ' '.join(fields[2:7])))
    kind, target = CONFIG['target_id'].split(':', 1)
    root_pid, _, started_at = target.partition(':')
    ids = {pid for pid, _, command, birth in rows if (command.startswith(target + '/') if kind == 'app' else pid == int(root_pid) and birth == started_at if kind == 'pid' else command == target)}
    while True:
        expanded = ids | {pid for pid, parent, _, _ in rows if parent in ids}
        if expanded == ids:
            return sorted(ids)
        ids = expanded


def loopback(address):
    if not address:
        return False
    host = address[0].strip('[]')
    if host.lower() == 'localhost':
        return True
    try:
        return ipaddress.ip_address(host).is_loopback
    except ValueError:
        return False


def owner_alive():
    owner = os.environ.get('MUX_CAPTURE_OWNER')
    if not owner:
        return False
    try:
        result = subprocess.run(['/bin/ps', '-p', owner, '-o', 'lstart='], capture_output=True, text=True, timeout=5)
        return result.returncode == 0 and ' '.join(result.stdout.split()) == os.environ.get('MUX_CAPTURE_OWNER_START')
    except (OSError, subprocess.TimeoutExpired):
        return False


class EventStream:
    """Forward untouched bytes; persist only complete, redacted SSE events."""
    def __init__(self, flow):
        self.flow = flow
        self.wire_bytes = 0
        self.decoded_bytes = 0
        self.pending = ''
        self.events = []
        self.failed = False
        self.truncated = False
        self.last_saved = 0.0
        self.decoder = codecs.getincrementaldecoder('utf-8')('strict')
        encoding = flow.response.headers.get('content-encoding', '').strip().lower()
        self.compression = zlib.decompressobj(16 + zlib.MAX_WBITS) if encoding == 'gzip' else zlib.decompressobj() if encoding == 'deflate' else None
        self.failed = encoding not in ('', 'identity', 'gzip', 'deflate')

    def consume(self, chunk):
        self.wire_bytes += len(chunk)
        if self.failed or self.truncated:
            return
        try:
            # Limit decompression before materialising a potentially huge body.
            decoded = self.compression.decompress(chunk, LIMIT - self.decoded_bytes + 1) if self.compression else chunk
            self.decoded_bytes += len(decoded)
            if self.decoded_bytes > LIMIT:
                self.truncated = True
                self.pending = ''
                self.events = []
                return
            self.pending += self.decoder.decode(decoded, final=not chunk)
            # Normalise line separators after decoding, including a CRLF split
            # across wire chunks. Never write an incomplete data frame.
            self.pending = self.pending.replace('\r\n', '\n')
            tail = '\r' if chunk and self.pending.endswith('\r') else ''
            self.pending = self.pending[:len(self.pending) - len(tail)].replace('\r', '\n') + tail
            while '\n\n' in self.pending:
                block, self.pending = self.pending.split('\n\n', 1)
                secrets = credentials(self.flow)
                self.events.append(redact_event(block, secrets))
        except (UnicodeDecodeError, zlib.error):
            self.failed = True
            self.events = []
            self.pending = ''

    def message(self):
        base = message(self.flow.response, credentials(self.flow))
        base['bytes'] = self.wire_bytes
        base['truncated'] = self.truncated or self.failed
        base['body'] = '[流式正文超过 2 MiB；未保存正文]' if self.truncated else '[无法安全解码流式正文；未保存正文]' if self.failed else ''.join(self.events)
        return base


class WebSocketStream:
    """Only redacted complete application messages; never raw binary frames."""
    def __init__(self, flow):
        self.flow = flow
        self.frames = {'request': [], 'response': []}
        self.bytes = {'request': 0, 'response': 0}
        self.secrets = set(credentials(flow))
        self.retained = 0
        self.count = 0
        self.truncated = False

    def consume(self, item):
        direction = 'request' if item.from_client else 'response'
        self.bytes[direction] += len(item.content)
        self.count += 1
        if self.count > 512:
            self.truncated = True
            return
        text = None
        omitted = not item.is_text or len(item.content) > LIMIT
        if not omitted:
            try:
                text = item.content.decode('utf-8')
                try:
                    value = json.loads(text)
                    for secret in credentials(self.flow, value):
                        if len(self.secrets) < 128:
                            self.secrets.add(secret)
                    text = json.dumps(redact(value), ensure_ascii=False)
                except (ValueError, RecursionError):
                    if text.lstrip().startswith(('{', '[')):
                        omitted = True
                    else:
                        text = re.sub(r'(?i)((?:password|secret|token|api[-_]?key)\s*[=:]\s*)[^\s&<]+', r'\1[REDACTED]', text)
                text = clean_text(text, sorted(self.secrets, key=len, reverse=True))
            except UnicodeDecodeError:
                omitted = True
        frame = {'sequence': self.count, 'timestamp': item.timestamp, 'bytes': len(item.content),
                 'type': 'text' if item.is_text else 'binary', 'text': None if omitted else text}
        if omitted:
            frame['omitted'] = True
            self.truncated = True
        size = len(json.dumps(frame, ensure_ascii=False).encode('utf-8'))
        if self.retained + size > LIMIT:
            self.truncated = True
            return
        self.retained += size
        self.frames[direction].append(frame)

    def message(self, direction, secrets):
        original = message(getattr(self.flow, direction), secrets)
        if original is None:
            return None
        original.update(body=json.dumps(self.frames[direction], ensure_ascii=False, indent=2),
                        bytes=original['bytes'] + self.bytes[direction],
                        truncated=self.truncated, encoding='text')
        return original


class Capture:
    def __init__(self):
        self.count = 0
        self.pids = CONFIG['pids']
        self.task = None
        self.warnings = []
        self.saved_bytes = 0
        self.inflight = set()
        self.saved_sizes = {}
        self.streams = {}
        self.websockets = {}
        self.websocket_timers = {}
        self.pending_flows = {}
        self.terminal_state = None

    def status(self, state, message=''):
        if state in ('stopped', 'failed'):
            self.terminal_state = state
        elif state == 'running' and self.terminal_state:
            state = self.terminal_state
        if message and message not in self.warnings:
            self.warnings.append(message)
        private_json(ROOT / 'status.json', {'state': state, 'message': ' '.join(self.warnings[-5:]), 'pids': self.pids, 'flow_count': self.count})

    async def running(self):
        self.status('running')
        self.task = asyncio.create_task(self.watch())

    async def watch(self):
        while True:
            await asyncio.sleep(2)
            owner = os.environ.get('MUX_CAPTURE_OWNER')
            if owner:
                if not await asyncio.to_thread(owner_alive):
                    self.status('stopped', 'MUX 已退出，抓包已停止。')
                    ctx.master.shutdown()
                    return
            if (ROOT / 'stop').exists():
                ctx.master.shutdown()
                return
            try:
                ids = await asyncio.to_thread(processes)
                if not ids:
                    self.status('stopped', '目标进程已退出；请重新选择正在运行的应用。')
                    ctx.master.shutdown()
                    return
                if ids != self.pids:
                    # SSE can remain open for hours. Do not postpone capture of
                    # new Agent helpers until every streaming request finishes.
                    # Update the existing redirector in place. Replacing the
                    # local mode instance stops/restarts its singleton and can
                    # clear interception while an existing SSE is still open.
                    redirector = mode_servers.LocalRedirectorInstance._server
                    if redirector is None or mode_servers.LocalRedirectorInstance._instance is None:
                        raise RuntimeError('Local redirector is unavailable')
                    redirector.set_intercept(','.join(map(str, ids)) + ',!' + str(os.getpid()))
                    self.pids = ids
                    self.status('running', '进程范围已更新；切换期间可能存在短暂间隙。')
            except Exception:
                self.status('failed', '无法更新拦截进程范围，抓包已停止。')
                ctx.master.shutdown()
                return

    async def dns_request(self, flow):
        if not CONFIG['proxy_url']:
            return
        self.inflight.add('dns:' + flow.id)
        try:
            def resolve():
                handler = urllib.request.ProxyHandler({'https': CONFIG['proxy_url']})
                client = urllib.request.build_opener(handler)
                request = urllib.request.Request('https://cloudflare-dns.com/dns-query', data=flow.request.packed, headers={'Content-Type': 'application/dns-message', 'Accept': 'application/dns-message'})
                with client.open(request, timeout=10) as response:
                    return dns.DNSMessage.unpack(response.read(65536))
            flow.response = await asyncio.to_thread(resolve)
        except Exception:
            flow.response = flow.request.fail(2)
            self.status('running', '通过所选代理解析 DNS 失败；未使用直连 DNS。')
        finally:
            self.inflight.discard('dns:' + flow.id)

    def tls_clienthello(self, data):
        address = data.context.server.address
        model_endpoint = address and any(
            address[1] == port and (address[0].strip('[]').lower() == host.lower() or host.lower() == 'localhost' and loopback(address))
            for host, port in CONFIG.get('loopback_tls', []))
        if loopback(address) and not model_endpoint:
            # Preserve the application's own TLS / IPC, including its certificate.
            data.ignore_connection = True

    def server_connect(self, data):
        if loopback(data.server.address):
            return
        # Fail closed: unsupported protocols must not bypass the chosen proxy.
        if CONFIG['proxy_url']:
            proxy = urlsplit(CONFIG['proxy_url'])
            if data.server.transport_protocol != 'tcp' or data.server.address != (proxy.hostname, proxy.port or 80):
                data.server.error = '所选 HTTP 出口不支持此连接；未回退直连。'
                self.status('running', '有连接未被 HTTP 出口支持；未回退直连。')

    def requestheaders(self, flow):
        self.inflight.add(flow.id)
        self.pending_flows[flow.id] = flow
        if loopback((flow.request.host, flow.request.port)):
            flow.server_conn.via = None
        elif CONFIG['proxy_url']:
            proxy = urlsplit(CONFIG['proxy_url'])
            flow.server_conn.via = ('http', (proxy.hostname, proxy.port or 80))
        else:
            flow.server_conn.via = None
        stream_body(flow, 'request')
        self.save(flow, 'pending')

    def request(self, flow):
        self.save(flow, 'pending')

    def responseheaders(self, flow):
        streaming = flow.response.headers.get('content-type', '').lower().startswith('text/event-stream')
        if not streaming and flow.response.status_code != 101:
            stream_body(flow, 'response')
        self.save(flow, 'streaming' if streaming else 'receiving')
        if streaming:
            stream = EventStream(flow)
            self.streams[flow.id] = stream
            def forward(chunk):
                stream.consume(chunk)
                now = time.monotonic()
                if not chunk or now - stream.last_saved >= 0.2:
                    stream.last_saved = now
                    self.save(flow, 'streaming')
                return chunk
            flow.response.stream = forward

    def response(self, flow):
        if flow.response.status_code == 101 and flow.response.headers.get('upgrade', '').lower() == 'websocket':
            self.websockets.setdefault(flow.id, WebSocketStream(flow))
            self.save(flow, 'streaming')
            return
        self.save(flow, 'complete')
        self.finish(flow.id)

    def websocket_start(self, flow):
        self.inflight.add(flow.id)
        self.pending_flows[flow.id] = flow
        self.websockets.setdefault(flow.id, WebSocketStream(flow))
        self.save(flow, 'streaming')

    def websocket_message(self, flow):
        stream = self.websockets.get(flow.id)
        if stream is None or not flow.websocket.messages:
            return
        stream.consume(flow.websocket.messages[-1])
        if flow.id not in self.websocket_timers:
            self.websocket_timers[flow.id] = asyncio.get_running_loop().call_later(0.15, self.flush_websocket, flow)
        # The forwarding layer owns the message object. Do not retain an
        # unbounded second raw transcript inside our private mitmdump engine.
        flow.websocket.messages.clear()

    def flush_websocket(self, flow):
        self.websocket_timers.pop(flow.id, None)
        if flow.id in self.websockets:
            self.save(flow, 'streaming')

    def websocket_end(self, flow):
        self.save(flow, 'failed' if flow.error else 'complete')
        self.finish(flow.id)

    def error(self, flow):
        reason = str(flow.error).lower()
        cancelled = any(fragment in reason for fragment in ('client disconnected', 'client disconnect', 'client closed', 'closed by client'))
        self.save(flow, 'cancelled' if cancelled else 'failed')
        self.finish(flow.id)

    def finish(self, flow_id):
        timer = self.websocket_timers.pop(flow_id, None)
        if timer:
            timer.cancel()
        self.inflight.discard(flow_id)
        flow = self.pending_flows.pop(flow_id, None)
        if flow:
            for direction in ('request', 'response'):
                msg = getattr(flow, direction)
                if msg is not None and isinstance(msg.stream, BodyStream):
                    msg.stream = True
        self.streams.pop(flow_id, None)
        self.websockets.pop(flow_id, None)

    def tls_failed_client(self, data):
        self.status('running', '客户端 TLS 握手失败；请检查抓包 CA 信任或证书固定。')

    def save(self, flow, phase):
        if flow.id not in self.saved_sizes and self.count >= MAX_FLOWS:
            self.status('stopped', '已达到 5000 条请求上限，请开始新的会话。')
            ctx.master.shutdown()
            return
        if not flow.request:
            return
        secrets = credentials(flow)
        response = self.streams[flow.id].message() if flow.id in self.streams else captured_message(flow, 'response', secrets)
        request = captured_message(flow, 'request', secrets)
        websocket = self.websockets.get(flow.id)
        if websocket:
            request = websocket.message('request', secrets)
            response = websocket.message('response', secrets)
        if response and phase == 'cancelled':
            response['truncated'] = True
        now = time.time()
        first_byte = flow.response.timestamp_start if flow.response else None
        terminal = phase in ('complete', 'failed', 'cancelled')
        ended_at = now if websocket else (flow.response.timestamp_end or now) if flow.response else now
        record = {
            'id': flow.id, 'method': flow.request.method, 'url': clean_text(clean_url(flow.request.pretty_url), secrets),
            'status': flow.response.status_code if flow.response else None,
            'started_at': flow.request.timestamp_start,
            'duration_ms': max(0, round(1000 * (ended_at - flow.request.timestamp_start))) if terminal else None,
            'error': ('连接或转发失败；未回退直连。' if CONFIG['proxy_url'] else '直连请求失败。') if flow.error and phase != 'cancelled' else None,
            'request': request, 'response': response,
            'phase': phase, 'updated_at': now,
            'request_bytes': request['bytes'] if request else 0,
            'response_bytes': response['bytes'] if response else 0,
            'time_to_first_byte_ms': max(0, round(1000 * (first_byte - flow.request.timestamp_start))) if first_byte else None,
            'content_type': clean_text(flow.response.headers.get('content-type', ''), secrets) if flow.response else None,
            'stream_type': 'websocket' if websocket else 'sse' if flow.id in self.streams else None,
            'request_http_version': flow.request.http_version,
            'response_http_version': flow.response.http_version if flow.response else None,
            'proxy_url': CONFIG['proxy_url'],
        }
        summary = {key: record[key] for key in ['id', 'method', 'url', 'status', 'started_at', 'duration_ms', 'error', 'phase', 'updated_at', 'request_bytes', 'response_bytes', 'time_to_first_byte_ms', 'content_type', 'stream_type']}
        size = len(json.dumps(record, ensure_ascii=False).encode('utf-8')) + len(json.dumps(summary, ensure_ascii=False).encode('utf-8'))
        next_size = self.saved_bytes - self.saved_sizes.get(flow.id, 0) + size
        if next_size > 100 * 1024 * 1024:
            self.status('stopped', '会话达到 100 MiB 保存上限，请开始新的会话。')
            ctx.master.shutdown()
            return
        self.saved_bytes = next_size
        self.saved_sizes[flow.id] = size
        self.count = len(self.saved_sizes)
        private_json(ROOT / 'flows' / (flow.id + '.json'), record)
        private_json(ROOT / 'summaries' / (flow.id + '.json'), summary)
        self.status('running')

    def done(self):
        if self.task:
            self.task.cancel()
        for timer in self.websocket_timers.values():
            timer.cancel()
        self.websocket_timers.clear()
        for flow in list(self.pending_flows.values()):
            self.save(flow, 'cancelled')
        self.pending_flows.clear()
        self.streams.clear()
        self.websockets.clear()
        if (ROOT / 'status.json').exists():
            previous = json.loads((ROOT / 'status.json').read_text())
            if previous['state'] == 'running':
                self.status('stopped')


addons = [Capture()]
