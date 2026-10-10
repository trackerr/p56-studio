#!/usr/bin/env python3
"""fan_emu.py - emulator of a Holoscope/P56 POV fan (cloud copy; see PROTOCOL.md)."""
import os, argparse, socket, threading, time, http.server, socketserver, re
import numpy as np, cv2
RAW = open('emu_raw.bin', 'ab')
from holofan import TYPES, ARMS, unlayout, FRAME_SIZE, CHUNK

class State:
    def __init__(s): s.lock = threading.Lock(); s.polar = None; s.frames = 0; s.bad = 0; s.info = 'waiting for pc...'; s.view = None; s.logged = 0; s.playlist = []; s.live_t = 0; s.files = {}; s.cur = 0; s.playing = True; s.flags = 0x1E; s.power = True

def decode(frame, ftype):
    if frame[:2] != b'\x5a\x28' or frame[-2:] != b'\x55\xf5':
        raise ValueError(f'bad header/footer: {frame[:2].hex()}..{frame[-2:].hex()}')
    end = frame.find(b'\xff\xd9', 2)
    if end < 0: raise ValueError('no JPEG end')
    mat = cv2.imdecode(np.frombuffer(frame[2:end+2], np.uint8), cv2.IMREAD_COLOR)
    if mat is None: raise ValueError('JPEG unreadable')
    angles, rad = TYPES[ftype]; arms = ARMS.get(ftype, 0)
    exp = (angles // max(arms, 1), rad * max(arms, 1)) if arms else (angles, rad)
    if mat.shape[:2] != exp:
        for t, (a, r) in TYPES.items():
            n = ARMS.get(t, 0)
            if mat.shape[:2] == ((a // n, r * n) if n else (a, r)):
                ftype, angles, rad, arms = t, a, r, n; break
        else:
            raise ValueError(f'unknown size {mat.shape[1]}x{mat.shape[0]}')
    return unlayout(mat, arms, rad), ftype, mat.shape, end - 1

NAME = b'EMU-P56'

def jpegs_in(data):
    out, i = [], data.find(b'\xff\xd8')
    while i >= 0:
        j = data.find(b'\xff\xd9', i)
        if j < 0: break
        out.append(data[i:j+2]); i = data.find(b'\xff\xd8', j+2)
    return out

def polar_from_jpeg(jpg, ftype):
    mat = cv2.imdecode(np.frombuffer(jpg, np.uint8), cv2.IMREAD_COLOR)
    if mat is None: return None, ftype
    for t in [ftype] + list(TYPES):
        a, r = TYPES[t]; n = ARMS.get(t, 0)
        if mat.shape[:2] == ((a // n, r * n) if n else (a, r)):
            return unlayout(mat, n, r), t
    return None, ftype

def reply_state(st, ftype):
    angles, rad = TYPES.get(ftype, TYPES[1])
    flags = st.flags if st.playing else (st.flags & ~0x0C)
    if not st.power: flags &= ~0x10
    return bytes([0x5A, 0x86, flags & 0xFF, st.cur & 0xFF, rad & 0xFF, angles >> 8, angles & 0xFF, 0,
                  ftype & 0xFF, 0, 0xF5])

def select_file(st, n):
    names = list(st.files)
    if 1 <= n <= len(names):
        st.cur, st.playing = n, True
        st.playlist = st.files[names[n-1]]
        st.info = f'PLAYING {n}: {names[n-1]} | {len(st.playlist)} frame(s)'
        return True
    return False

def reply_info():
    r = bytearray(126); r[0], r[1], r[125] = 0x5A, 0x8D, 0xF5
    r[14:14+len(NAME)] = NAME; r[46:52] = bytes.fromhex('02e0e0e0e001')
    r[52:52+8] = b'EMU-WIFI'; r[84:84+5] = b'v0.1e'
    return bytes(r)

READY = bytes([0x5A, 0x0E, 0x0E, 0xF5])

def reply_filelist(names):
    b = '/'.join(f'{i} {n}' for i, n in enumerate(names, 1)).encode('ascii', 'replace')[:2000]
    body = bytes([len(b) >> 8, len(b) & 0xFF]) + b
    x = 0
    for v in body: x ^= v
    return bytes([0x5A, 0x84]) + body + bytes([x, 0xF5])

def later(c, data, delay=0.25):
    def go():
        try: c.sendall(data)
        except OSError: pass
    threading.Timer(delay, go).start()

def run_link(st, pc_ip, ftype, ack):
    while True:
        try:
            c = socket.create_connection((pc_ip, 6666), timeout=5); c.settimeout(None)
            st.info = f'connected to {pc_ip}:6666'; print('[emu]', st.info, flush=True)
            buf, half, up = bytearray(), False, None
            def log(*m): print('[emu]', *m, flush=True)
            while True:
                d = c.recv(65536)
                if not d: raise ConnectionError('pc closed the connection')
                RAW.write(d); RAW.flush(); buf += d
                while buf:
                    if up is not None:
                        if up['size'] is None:
                            if len(buf) < 8: break
                            up['tag'] = int.from_bytes(buf[0:4], 'little'); up['size'] = int.from_bytes(buf[4:8], 'little')
                            del buf[:8]; log(f"upload {up['name']}: {up['size']} B (tag {up['tag']:#x})")
                        if len(buf) < up['size']: break
                        content = bytes(buf[:up['size']]); del buf[:up['size']]
                        open('emu_' + up['name'].replace('/', '_') + '.bin', 'wb').write(content)
                        frames = []
                        for jpg in jpegs_in(content):
                            pol, t = polar_from_jpeg(jpg, ftype)
                            if pol is not None: frames.append(pol)
                        log(f"upload done: {up['name']}, {len(frames)} frame(s)")
                        with st.lock:
                            st.files[up['name']] = frames
                            select_file(st, list(st.files).index(up['name']) + 1)
                            if frames:
                                st.playlist = frames
                                st.info = f"FILE {up['name']} | {len(frames)} frame(s) | {len(content)} B"
                        c.sendall(READY); later(c, reply_filelist(list(st.files)), 0.5); up = None
                        continue
                    if buf[0] == 0x5A and len(buf) >= 2 and buf[1] == 0x28:
                        if len(buf) >= CHUNK and not half: c.sendall(ack); half = True
                        if len(buf) < FRAME_SIZE: break
                        fr = bytes(buf[:FRAME_SIZE]); del buf[:FRAME_SIZE]
                        c.sendall(ack); half = False
                        try:
                            polar, t, shp, jl = decode(fr, ftype)
                            with st.lock:
                                st.polar = polar; st.frames += 1; st.live_t = time.time()
                                st.info = f'LIVE type {t} | {shp[1]}x{shp[0]} | jpeg {jl} B | frames {st.frames} | bad {st.bad}'
                        except ValueError as e:
                            st.bad += 1; log('frame error:', e)
                    elif buf[0] == 0x5A:
                        end = buf.find(b'\xf5', 2)
                        if end < 0:
                            if len(buf) > 4096: log('no F5, discarding:', buf[:32].hex(' ')); buf.clear()
                            break
                        pkt = bytes(buf[:end+1]); del buf[:end+1]
                        if len(pkt) == 4 and buf[:1] != b'\x5a' and len(buf) >= 1: del buf[:1]
                        cmd = pkt[1]
                        if cmd == 0x84 and len(pkt) > 5:
                            n = pkt[2]; name = pkt[3:3+n].decode('ascii', 'replace')
                            up = {'name': name, 'size': None}
                            log(f'cmd 84 upload start: {name!r}')
                            c.sendall(READY); continue
                        if cmd == 0x0D:   c.sendall(reply_state(st, ftype))
                        elif cmd == 0x87:
                            with st.lock: ok = select_file(st, pkt[2])
                            log(f'cmd 87: select file {pkt[2]} -> {"ok" if ok else "does not exist"}')
                            c.sendall(READY); later(c, reply_state(st, ftype), 0.3)
                        elif cmd in (0x01, 0x02, 0x03, 0x04):
                            with st.lock:
                                if cmd == 0x01: st.power = True
                                if cmd == 0x02: st.power = False
                                if cmd == 0x03: st.playing = False
                                if cmd == 0x04: st.playing = True; st.power = True
                            log(f'cmd {cmd:02X}: {["","ON","OFF","PAUSE","PLAY"][cmd]}')
                            c.sendall(READY); later(c, reply_state(st, ftype), 0.3)
                        elif cmd == 0x80:
                            names = list(st.files); n = pkt[2]
                            with st.lock:
                                if 1 <= n <= len(names):
                                    gone = names[n-1]; del st.files[gone]
                                    try: os.remove('emu_' + gone + '.bin')
                                    except OSError: pass
                                    if st.cur == n: st.cur, st.playlist = 0, []
                                    elif st.cur > n: st.cur -= 1
                                else: gone = None
                            log(f'cmd 80: delete file {n} -> {gone}')
                            c.sendall(READY); later(c, reply_filelist(list(st.files)), 0.3)
                        elif cmd == 0x1C:
                            c.sendall(reply_info()); later(c, reply_filelist(list(st.files)), 0.5); log(f'1C get machine -> 8D info + list {list(st.files)}')
                        else:
                            log(f'cmd {cmd:02X}: {pkt.hex(" ")}  -> 0E ready + list')
                            c.sendall(READY); later(c, reply_filelist(list(st.files)))
                            if cmd in (0x91, 0x8E, 0x10): later(c, reply_state(st, ftype), 0.8)
                    else:
                        log('unknown data:', bytes(buf[:32]).hex(' ')); buf.clear(); break
        except OSError as e:
            st.info = f'no connection to {pc_ip}:6666 ({e.__class__.__name__}), retrying...'
            print('[emu]', st.info, flush=True); time.sleep(2)

def discovery(st, ftype, ack, fixed_pc):
    if fixed_pc:
        run_link(st, fixed_pc, ftype, ack); return
    u = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    u.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1); u.setsockopt(socket.SOL_SOCKET, socket.SO_BROADCAST, 1)
    u.bind(('', 8988)); started = set()
    print('[emu] waiting for a broadcast on UDP 8988...', flush=True)
    while True:
        d, a = u.recvfrom(2048)
        m = re.fullmatch(rb'(\d+\.\d+\.\d+\.\d+)HS', d.strip())
        if not m: continue
        pc = m.group(1).decode()
        me = socket.gethostbyname(socket.gethostname())
        u.sendto(f'{me}HS'.encode(), (a[0], 8988))
        if pc not in started:
            started.add(pc); print(f'[emu] pc found: {pc}', flush=True)
            threading.Thread(target=run_link, args=(st, pc, ftype, ack), daemon=True).start()

PANEL = 480
_MASK = {}
def inverse(polar, size=PANEL):
    img = cv2.warpPolar(polar, (size, size), (size / 2, size / 2), size / 2, cv2.INTER_LINEAR + cv2.WARP_INVERSE_MAP)
    if size not in _MASK:
        m = np.zeros((size, size), np.uint8); cv2.circle(m, (size // 2, size // 2), size // 2, 255, -1); _MASK[size] = m
    return cv2.bitwise_and(img, img, mask=_MASK[size])

class Renderer:
    def __init__(s, rps, decay): s.rps, s.decay, s.acc, s.theta, s.t = rps, decay, None, 0.0, time.time()
    def render(s, polar, arms, info):
        now = time.time(); dt, s.t = now - s.t, now
        if polar is None:
            v = np.zeros((PANEL + 30, PANEL * 2, 3), np.uint8)
            cv2.putText(v, info, (10, PANEL // 2), cv2.FONT_HERSHEY_SIMPLEX, 0.6, (200, 200, 200), 1); return v
        ang = polar.shape[0]; arms = max(arms, 1)
        if s.acc is None or s.acc.shape != polar.shape: s.acc = np.zeros(polar.shape, np.float32)
        dtheta = s.rps * dt * ang; s.acc *= s.decay ** (dt * 30)
        for k in range(arms):
            a0 = s.theta + k * ang / arms
            rows = np.arange(int(a0), int(a0 + dtheta) + 1) % ang; s.acc[rows] = polar[rows]
        s.theta = (s.theta + dtheta) % ang
        left = inverse(polar); right = inverse(np.clip(s.acc, 0, 255).astype(np.uint8)); c = PANEL // 2
        for k in range(arms):
            phi = 2 * np.pi * ((s.theta + k * ang / arms) % ang) / ang
            cv2.line(right, (c, c), (int(c + c * np.cos(phi)), int(c + c * np.sin(phi))), (80, 80, 80), 2)
        for img in (left, right): cv2.circle(img, (c, c), c - 1, (60, 60, 60), 1)
        v = np.zeros((PANEL + 30, PANEL * 2, 3), np.uint8); v[:PANEL, :PANEL] = left; v[:PANEL, PANEL:] = right
        cv2.putText(v, info, (8, PANEL + 21), cv2.FONT_HERSHEY_SIMPLEX, 0.5, (200, 200, 200), 1)
        cv2.putText(v, 'eye (full speed)', (8, 18), cv2.FONT_HERSHEY_SIMPLEX, 0.5, (150, 150, 150), 1)
        cv2.putText(v, f'slow motion {s.rps} rev/s', (PANEL + 8, 18), cv2.FONT_HERSHEY_SIMPLEX, 0.5, (150, 150, 150), 1)
        return v

PAGE = b'''<!doctype html><html><head><meta name="viewport" content="width=device-width,initial-scale=1"><title>Fan emulator</title>
<style>body{margin:0;background:#000;color:#aaa;font:14px sans-serif;text-align:center}img{max-width:100%;height:auto}</style></head>
<body><img src="/stream"><p>Holoscope POV fan emulator</p></body></html>'''

def http_server(st, port):
    class H(http.server.BaseHTTPRequestHandler):
        def log_message(self, *a): pass
        def do_GET(self):
            if self.path == '/stream':
                self.send_response(200); self.send_header('Content-Type', 'multipart/x-mixed-replace; boundary=f'); self.end_headers()
                try:
                    while True:
                        j = st.view
                        if j: self.wfile.write(b'--f\r\nContent-Type: image/jpeg\r\nContent-Length: %d\r\n\r\n' % len(j) + j + b'\r\n')
                        time.sleep(1 / 25)
                except OSError: return
            elif self.path.startswith('/snap'):
                j = st.view or b''; self.send_response(200); self.send_header('Content-Type', 'image/jpeg'); self.end_headers(); self.wfile.write(j)
            else:
                self.send_response(200); self.send_header('Content-Type', 'text/html'); self.end_headers(); self.wfile.write(PAGE)
    socketserver.ThreadingTCPServer.allow_reuse_address = True
    srv = socketserver.ThreadingTCPServer(('', port), H); srv.daemon_threads = True
    print(f'[emu] view: http://localhost:{port}/', flush=True); srv.serve_forever()

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--pc'); ap.add_argument('--type', type=int, default=8); ap.add_argument('--ack', default='01')
    ap.add_argument('--port', type=int, default=8099); ap.add_argument('--rps', type=float, default=1.0)
    ap.add_argument('--decay', type=float, default=0.93); ap.add_argument('--window', action='store_true'); ap.add_argument('--save')
    a = ap.parse_args(); st = State()
    import glob
    for fn in sorted(glob.glob('emu_*.mp4.bin'), key=os.path.getmtime):
        name = os.path.basename(fn)[4:-4]
        frames = [p for p in (polar_from_jpeg(j, a.type)[0] for j in jpegs_in(open(fn, 'rb').read())) if p is not None]
        st.files[name] = frames
        if frames: st.playlist = frames; st.cur = len(st.files)
    if st.files: print(f'[emu] {len(st.files)} file(s) loaded: {list(st.files)}', flush=True)
    threading.Thread(target=discovery, args=(st, a.type, bytes.fromhex(a.ack), a.pc), daemon=True).start()
    threading.Thread(target=http_server, args=(st, a.port), daemon=True).start()
    r, last_save = Renderer(a.rps, a.decay), 0
    while True:
        with st.lock:
            polar, info = st.polar, st.info
            if st.playlist and time.time() - st.live_t > 1:
                polar = st.playlist[int(time.time() * 25) % len(st.playlist)]
                if not st.power: polar = np.zeros_like(polar); info = 'OFF | ' + info
                elif not st.playing: polar = np.zeros_like(polar); info = 'PAUSED | ' + info
        v = r.render(polar, ARMS.get(a.type, 0), info)
        st.view = cv2.imencode('.jpg', v, [cv2.IMWRITE_JPEG_QUALITY, 85])[1].tobytes()
        if a.save and time.time() - last_save > 1: cv2.imwrite(a.save, v); last_save = time.time()
        time.sleep(1 / 30)

if __name__ == '__main__':
    try: main()
    except KeyboardInterrupt: pass
