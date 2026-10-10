#!/usr/bin/env python3
"""holofan.py - reference transcode + cast (copy from the laptop, see PROTOCOL.md)."""
import argparse, socket, threading, time, select, sys
import numpy as np, cv2

UDP_PORT, TCP_PORT = 8988, 6666
FRAME_SIZE, MAX_JPEG, CHUNK = 0x5000, 0x4FFC, 0x3908
TYPES = {1:(960,192),2:(960,168),3:(1440,240),4:(960,144),5:(960,128),6:(600,133),
         7:(490,122),8:(960,172),11:(960,184),12:(512,85),13:(490,128),14:(960,176),
         15:(490,122),17:(960,192)}
ARMS = {**{t:4 for t in (1,2,4,8,11)}, **{t:2 for t in (5,14,17)}, 3:1}
ORDER = {4: [0, 3, 2, 1], 2: [0, 1]}
B70 = {**{t:1.5 for t in (1,2,3,4,5,8,11,17)}, 6:2.0, 7:1.6, 15:1.6, 12:1.6, 13:1.6, 14:1.4}
RADIAL = np.array([0.73]*19 + [0.74]*5 + [0.77]*3 + [0.80]*3 + [0.83]*3 + [0.86]*3 +
                  [0.89]*3 + [0.92]*3 + [0.95]*3 + [0.98]*3)

def auto_brightness(img, ftype):
    mean = cv2.mean(cv2.cvtColor(img, cv2.COLOR_BGR2GRAY))[0]
    if mean < 1: return img
    g = mean * B70.get(ftype, 1.5)
    g = min(g, 100.0) if ftype == 14 else min(g, 150.0)
    return cv2.convertScaleAbs(img, alpha=g / mean)

def radial_correction(polar, ftype):
    ang, rad = polar.shape[:2]
    polar = polar[:, :, ::-1]
    flat = polar.reshape(ang, rad * 3).astype(np.float64)
    f = RADIAL.copy()
    if ftype in (5, 14, 17): f[:3] = 0.6
    n = min(len(f), flat.shape[1])
    flat[:, :n] = np.trunc(flat[:, :n] * f[:n])
    k = (np.arange(ang) % 8) * 3
    cols = np.arange(rad * 3)
    flat[cols[None, :] < k[:, None]] = 0
    return np.ascontiguousarray(flat.astype(np.uint8).reshape(ang, rad, 3)[:, :, ::-1])

def to_polar(img, angles, radius_px):
    h, w = img.shape[:2]
    R = min(w, h) // 2 + 0.5
    return cv2.warpPolar(img, (radius_px, angles), (w // 2, h // 2), R,
                         cv2.INTER_LANCZOS4 + cv2.WARP_FILL_OUTLIERS)

def layout(polar, arms):
    ang = polar.shape[0]
    if arms == 4:
        q = ang // 4; order = ORDER[4]
    elif arms == 2:
        q = ang // 2; order = ORDER[2]
    elif arms == 1:
        return np.ascontiguousarray(np.flipud(polar[:, ::-1, ::-1]))
    else:
        return polar
    rows = [np.concatenate([polar[k*q + i][::-1] for k in order], axis=0) for i in range(q)]
    return np.ascontiguousarray(np.flipud(np.stack(rows)))

def unlayout(mat, arms, rad):
    if arms == 1:
        return np.ascontiguousarray(np.flipud(mat)[:, ::-1, ::-1])
    if arms not in ORDER:
        return mat
    rows = np.flipud(mat); q = rows.shape[0]
    polar = np.zeros((q * arms, rad, 3), np.uint8)
    for j, k in enumerate(ORDER[arms]):
        polar[k*q:(k+1)*q] = rows[:, j*rad:(j+1)*rad][:, ::-1]
    return polar

def encode(img, ftype=8, quality=0.8):
    angles, rad = TYPES.get(ftype, TYPES[1])
    polar = radial_correction(to_polar(auto_brightness(img, ftype), angles, rad), ftype)
    mat = layout(polar, ARMS.get(ftype, 0))
    for _ in range(11):
        ok, jpg = cv2.imencode('.jpg', mat, [cv2.IMWRITE_JPEG_QUALITY, int(quality*100)])
        if len(jpg) <= MAX_JPEG: break
        quality *= 0.9
    jpg = jpg.tobytes()[:MAX_JPEG]
    buf = bytearray(FRAME_SIZE)
    buf[0:2] = b'\x5a\x28'; buf[2:2+len(jpg)] = jpg; buf[-2:] = b'\x55\xf5'
    return bytes(buf), mat

def test_image():
    img = np.zeros((720, 720, 3), np.uint8)
    for r, col in ((340,(0,0,255)),(250,(0,255,0)),(160,(255,0,0))):
        cv2.circle(img, (360,360), r, col, 25)
    cv2.line(img, (360,0), (360,360), (255,255,255), 15)
    return img
