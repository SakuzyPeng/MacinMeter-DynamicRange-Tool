"""Print controlled static-analysis evidence from the active IDA database.

Run under IDA, not the system Python. Decompiled text is diagnostic output and
must not be committed; public evidence records should contain only independently
written findings, function identities, and hashes.
"""

from __future__ import annotations

import hashlib
from pathlib import Path

import ida_auto
import ida_funcs
import ida_hexrays
import ida_ida
import ida_idaapi
import ida_kernwin
import ida_name
import ida_nalt
import ida_pro
import idautils


TARGET_NAMES = (
    "sub_180008410",
    "sub_1800089F0",
    "sub_180008DF0",
    "sub_18000A280",
    "sub_18003E880",
    "sub_18003F120",
    "sub_180044470",
)

INTERESTING_STRING_PATTERNS = (
    "official dr",
    "number of tracks",
    "weight album",
    "weight multichannel",
    "channel loudness",
    "track lengths",
    "per-channel stats",
    "automatically save tags",
)


def resolve_function(name: str) -> int | None:
    address = ida_name.get_name_ea(ida_idaapi.BADADDR, name)
    function = ida_funcs.get_func(address)
    return None if function is None else function.start_ea


def print_function(address: int, label: str) -> None:
    name = ida_funcs.get_func_name(address)
    print(f"FUNCTION_BEGIN={label}:{name}:{address:#x}")
    function = ida_hexrays.decompile(address)
    if function is None:
        print("DECOMPILATION_FAILED")
    else:
        print(function)
    print(f"FUNCTION_END={label}:{name}")


def print_target_callers(name: str, address: int) -> None:
    seen: set[int] = set()
    for xref in idautils.XrefsTo(address):
        function = ida_funcs.get_func(xref.frm)
        if function is None or function.start_ea in seen:
            continue
        seen.add(function.start_ea)
        print(
            "TARGET_CALLER="
            f"{name}:{xref.frm:#x}:{ida_funcs.get_func_name(function.start_ea)}:"
            f"{function.start_ea:#x}"
        )


def print_interesting_string_callers() -> None:
    callers: dict[int, set[str]] = {}
    for item in idautils.Strings():
        value = str(item)
        folded = value.casefold()
        if not any(pattern in folded for pattern in INTERESTING_STRING_PATTERNS):
            continue
        print(f"INTERESTING_STRING={item.ea:#x}:{value!r}")
        for xref in idautils.XrefsTo(item.ea):
            function = ida_funcs.get_func(xref.frm)
            if function is None:
                continue
            callers.setdefault(function.start_ea, set()).add(value)
            print(
                "STRING_XREF="
                f"{item.ea:#x}:{xref.frm:#x}:"
                f"{ida_funcs.get_func_name(function.start_ea)}:{function.start_ea:#x}"
            )

    for address, values in sorted(callers.items()):
        print(f"STRING_CALLER_CONTEXT={address:#x}:{sorted(values)!r}")
        print_target_callers(ida_funcs.get_func_name(address), address)
        print_function(address, "string-caller")


def main() -> None:
    ida_auto.auto_wait()
    input_path = Path(ida_nalt.get_input_file_path())
    print(f"IDA_VERSION={ida_kernwin.get_kernel_version()}")
    print(f"INPUT_SHA256={hashlib.sha256(input_path.read_bytes()).hexdigest()}")
    print(f"IS_64_BIT={ida_ida.inf_is_64bit()}")

    for name in TARGET_NAMES:
        address = resolve_function(name)
        if address is None:
            print(f"FUNCTION_MISSING={name}")
            continue
        print_target_callers(name, address)
        print_function(address, name)

    print_interesting_string_callers()


try:
    main()
finally:
    ida_pro.qexit(0)
