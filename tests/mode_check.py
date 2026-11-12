#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""mode_check.py —— 模拟客户端校验"加密方式不匹配向对方报错"。

用法: python3 mode_check.py <host> <port> <register_mode>
- register_mode=plain：用明文 WS 注册（连明文主节点）
- register_mode=tls  ：注册帧声明 tls（连明文主节点，制造声明不匹配）
退出码 0 = 报错语义正确；非 0 = 失败。
"""
import base64
import json
import os
import socket
import struct
import sys


def recv_frame_plain(sock):
    """服务端→客户端帧不带掩码（RFC6455）。"""
    h = sock.recv(2)
    if len(h) < 2:
        return ""
    n = h[1] & 0x7F
    if n == 126:
        (n,) = struct.unpack(">H", sock.recv(2))
    elif n == 127:
        (n,) = struct.unpack(">Q", sock.recv(8))
    data = b""
    while len(data) < n:
        chunk = sock.recv(n - len(data))
        if not chunk:
            break
        data += chunk
    return data.decode("utf-8", "replace")


def main():
    host, port, mode = sys.argv[1], int(sys.argv[2]), sys.argv[3]
    s = socket.create_connection((host, port), timeout=10)
    key = base64.b64encode(os.urandom(16)).decode()
    s.sendall(
        (
            "GET / HTTP/1.1\r\nHost: x\r\nUpgrade: websocket\r\n"
            "Connection: Upgrade\r\n"
            f"Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
        ).encode()
    )
    buf = b""
    while b"\r\n\r\n" not in buf:
        buf += s.recv(4096)

    # 带掩码的客户端文本帧
    def send_text(text):
        mask = os.urandom(4)
        payload = text.encode()
        if len(payload) < 126:
            head = bytes([0x81, 0x80 | len(payload)]) + mask
        else:
            head = bytes([0x81, 0x80 | 126]) + struct.pack(">H", len(payload)) + mask
        s.sendall(head + bytes(b ^ mask[i % 4] for i, b in enumerate(payload)))

    send_text(json.dumps({"action": "register", "from": "fake-check", "mode": mode, "ver": 2}))
    reply = recv_frame_plain(s)
    if "mode_mismatch" in reply:
        print("mode_mismatch 报错符合预期:", reply)
        return 0
    print("未收到 mode_mismatch，实际回执:", reply)
    return 1


if __name__ == "__main__":
    sys.exit(main())
