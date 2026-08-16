"""mDNS service registration for LocalDub servers.

Usage:
    from mdns_server import register_service, unregister_service

    reg = register_service("voxcpm_torch_gradio", port=19112)
    # ... server runs ...
    unregister_service(reg)
"""

from __future__ import annotations

import os
import socket
import sys
from typing import Any

SERVICE_TYPES = {
    "server": "_ld-server._tcp.local.",
    "voxcpm_torch_gradio": "_ld-voxcpm-py._tcp.local.",
}

def _hostname() -> str:
    return socket.gethostname()


def _local_ip() -> str:
    """Best-effort local IP for mDNS advertisement."""
    try:
        s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        s.settimeout(0.1)
        s.connect(("10.255.255.255", 1))
        ip = s.getsockname()[0]
        s.close()
        return ip
    except Exception:
        return "127.0.0.1"


def register_service(service_type: str, port: int) -> Any | None:
    """Register a mDNS service. Returns the Zeroconf instance for later cleanup."""
    if service_type not in SERVICE_TYPES:
        return None
    try:
        from zeroconf import IPVersion, ServiceInfo, Zeroconf
    except ImportError:
        print("[mDNS] zeroconf not installed, skipping", file=sys.stderr)
        return None

    hostname = _hostname()
    ip = _local_ip()

    info = ServiceInfo(
        type_=SERVICE_TYPES[service_type],
        name=f"{service_type}@{hostname}.{SERVICE_TYPES[service_type]}",
        addresses=[socket.inet_aton(ip)],
        port=port,
        properties={"pid": str(os.getpid()), "hostname": hostname},
        server=f"{hostname}.local.",
    )

    zc = Zeroconf(ip_version=IPVersion.V4Only)
    try:
        zc.register_service(info)
        print(f"[mDNS] Registered {service_type} on port {port} ({ip})", file=sys.stderr)
        return zc
    except Exception as e:
        print(f"[mDNS] Failed to register {service_type}: {e}", file=sys.stderr)
        zc.close()
        return None


def unregister_service(zc: Any | None) -> None:
    """Unregister and close a Zeroconf instance."""
    if zc is not None:
        try:
            zc.unregister_all_services()
            zc.close()
        except Exception:
            pass
