#!/usr/bin/env python3
"""
Modify Qualcomm Primary GPT image (gpt_main4.bin style, 4 KiB sectors).

Removes the active slot's vbmeta_* and pvmfw_* partition entries, compacts
the entry table, and recalculates GPT header / partition-array CRC32.

Matches the layout produced by gpt_main4_modify_v3.bin (slot A):
  - delete vbmeta_a, pvmfw_a
  - compact remaining entries (no holes)
  - update CRCs

Usage:
  python gpt_remove_slot_vbmeta.py gpt_main4.bin --slot a
  python gpt_remove_slot_vbmeta.py gpt_main4.bin --slot b -o gpt_main4_modify_b.bin
"""

from __future__ import annotations

import argparse
import struct
import sys
import zlib
from pathlib import Path

GPT_SIGNATURE = b"EFI PART"
DEFAULT_HEADER_OFFSET = 0x1000  # LBA 1 @ 4096-byte sectors in 24 KiB Primary GPT blob


def crc32_gpt(data: bytes) -> int:
    return zlib.crc32(data) & 0xFFFFFFFF


def parse_header(data: bytes, header_offset: int) -> dict:
    if data[header_offset : header_offset + 8] != GPT_SIGNATURE:
        raise ValueError(
            f"EFI PART signature not found at offset {header_offset:#x}"
        )

    hdr_size = struct.unpack_from("<I", data, header_offset + 12)[0]
    if hdr_size < 92:
        raise ValueError(f"Unexpected GPT header size: {hdr_size}")

    return {
        "offset": header_offset,
        "size": hdr_size,
        "part_entry_lba": struct.unpack_from("<Q", data, header_offset + 72)[0],
        "num_entries": struct.unpack_from("<I", data, header_offset + 80)[0],
        "entry_size": struct.unpack_from("<I", data, header_offset + 84)[0],
    }


def entry_name(entry: bytes) -> str:
    return entry[56:128].decode("utf-16-le", errors="replace").rstrip("\x00")


def entry_used(entry: bytes) -> bool:
    if entry[:16] == b"\x00" * 16:
        return False
    first, last = struct.unpack_from("<QQ", entry, 32)
    return not (first == 0 and last == 0)


def read_used_entries(data: bytes, hdr: dict) -> list[tuple[str, bytes]]:
    base = hdr["part_entry_lba"] * 4096
    esz = hdr["entry_size"]
    num = hdr["num_entries"]
    entries: list[tuple[str, bytes]] = []

    for i in range(num):
        off = base + i * esz
        ent = data[off : off + esz]
        if len(ent) < esz:
            break
        if entry_used(ent):
            entries.append((entry_name(ent), bytes(ent)))

    return entries


def remove_targets(slot: str) -> set[str]:
    slot = slot.lower()
    if slot not in ("a", "b"):
        raise ValueError("slot must be 'a' or 'b'")
    suffix = f"_{slot}"
    return {f"vbmeta{suffix}", f"pvmfw{suffix}"}


def modify_gpt(data: bytes, slot: str, header_offset: int = DEFAULT_HEADER_OFFSET) -> bytes:
    hdr = parse_header(data, header_offset)
    remove = remove_targets(slot)

    used = read_used_entries(data, hdr)
    names = [n for n, _ in used]
    missing = remove - set(names)
    if missing:
        raise ValueError(
            "Partition(s) not found in GPT: "
            + ", ".join(sorted(missing))
            + f"\nPresent vbmeta/pvmfw: "
            + ", ".join(n for n in names if "vbmeta" in n or "pvmfw" in n)
        )

    kept = [ent for name, ent in used if name not in remove]

    base = hdr["part_entry_lba"] * 4096
    esz = hdr["entry_size"]
    num = hdr["num_entries"]
    arr_size = num * esz

    if len(kept) > num:
        raise ValueError(
            f"Compacted entry count {len(kept)} exceeds table size {num}"
        )

    out = bytearray(data)
    new_arr = bytearray(arr_size)
    for i, ent in enumerate(kept):
        new_arr[i * esz : (i + 1) * esz] = ent
    out[base : base + arr_size] = new_arr

    hoff = hdr["offset"]
    arr_crc = crc32_gpt(bytes(out[base : base + arr_size]))
    struct.pack_into("<I", out, hoff + 88, arr_crc)

    header_bytes = bytearray(out[hoff : hoff + hdr["size"]])
    header_bytes[16:20] = b"\x00\x00\x00\x00"
    hdr_crc = crc32_gpt(bytes(header_bytes))
    struct.pack_into("<I", out, hoff + 16, hdr_crc)

    return bytes(out)


def main() -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Remove active-slot vbmeta_* and pvmfw_* from a Primary GPT blob "
            "(e.g. gpt_main4.bin) and refresh CRCs."
        )
    )
    parser.add_argument(
        "input",
        type=Path,
        help="Source Primary GPT file (typically gpt_main4.bin, 24 KiB)",
    )
    parser.add_argument(
        "--slot",
        required=True,
        choices=("a", "b"),
        help="Active slot: removes vbmeta_<slot> and pvmfw_<slot>",
    )
    parser.add_argument(
        "-o",
        "--output",
        type=Path,
        help="Output file (default: <input_stem>_modify_<slot>.bin)",
    )
    parser.add_argument(
        "--header-offset",
        type=lambda x: int(x, 0),
        default=DEFAULT_HEADER_OFFSET,
        help=f"Byte offset of EFI PART header (default: {DEFAULT_HEADER_OFFSET:#x})",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="List partitions to remove/keep without writing output",
    )
    args = parser.parse_args()

    if not args.input.is_file():
        print(f"error: input not found: {args.input}", file=sys.stderr)
        return 1

    data = args.input.read_bytes()
    remove = remove_targets(args.slot)

    try:
        hdr = parse_header(data, args.header_offset)
        used = read_used_entries(data, hdr)
    except ValueError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1

    print(f"Input:       {args.input} ({len(data)} bytes)")
    print(f"Slot:        {args.slot.upper()}  -> remove {sorted(remove)}")
    print(f"Entries:     {len(used)} used / {hdr['num_entries']} table slots")

    removed = [n for n, _ in used if n in remove]
    kept_names = [n for n, _ in used if n not in remove]
    print(f"Removing:    {', '.join(removed)}")
    print(f"After:       {len(kept_names)} entries")

    if args.dry_run:
        for n in kept_names:
            if "vbmeta" in n or "pvmfw" in n:
                print(f"  kept (other slot): {n}")
        return 0

    try:
        out = modify_gpt(data, args.slot, args.header_offset)
    except ValueError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1

    output = args.output
    if output is None:
        output = args.input.with_name(f"{args.input.stem}_modify_{args.slot}.bin")

    output.write_bytes(out)
    print(f"Written:     {output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
