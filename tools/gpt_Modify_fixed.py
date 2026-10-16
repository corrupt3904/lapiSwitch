#!/usr/bin/env python3
"""GPT: remove vbmeta_a, plus vbmeta and the active slot pvmfw if present.

The original user-supplied gpt_Modify.py is retained as a legacy reference.
Its parsing helpers are reused here; its active-slot modification is not used.
"""
from __future__ import annotations
import argparse
import struct
from pathlib import Path
from gpt_Modify import DEFAULT_HEADER_OFFSET, crc32_gpt, parse_header, read_used_entries


def modify_gpt(data: bytes, slot: str) -> bytes:
    if slot not in {'a', 'b'}:
        raise ValueError('Active slot must be a or b')
    h = parse_header(data, DEFAULT_HEADER_OFFSET)
    base = h['part_entry_lba'] * 4096
    array_size = h['num_entries'] * h['entry_size']
    hoff = h['offset']
    if h['size'] > 4096 or h['entry_size'] < 128 or base + array_size > len(data):
        raise ValueError('Invalid or truncated GPT')
    header = bytearray(data[hoff:hoff + h['size']])
    original_crc = struct.unpack_from('<I', header, 16)[0]
    header[16:20] = bytes(4)
    if crc32_gpt(header) != original_crc:
        raise ValueError('Invalid header CRC')
    if crc32_gpt(data[base:base + array_size]) != struct.unpack_from('<I', data, hoff + 88)[0]:
        raise ValueError('Invalid partition array CRC')
    used = read_used_entries(data, h)
    if not any(name == 'vbmeta_a' for name, _ in used):
        raise ValueError('Required partition not found: vbmeta_a')
    kept = [entry for name, entry in used if name not in {'vbmeta_a', 'vbmeta', f'pvmfw_{slot}'}]
    array = b''.join(kept).ljust(array_size, b'\0')
    out = bytearray(data)
    out[base:base + array_size] = array
    struct.pack_into('<I', out, hoff + 88, crc32_gpt(array))
    header = bytearray(out[hoff:hoff + h['size']])
    header[16:20] = bytes(4)
    struct.pack_into('<I', out, hoff + 16, crc32_gpt(header))
    return bytes(out)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('input', type=Path)
    parser.add_argument('-o', '--output', type=Path)
    parser.add_argument('--slot', required=True, choices=['a', 'b'], help='active boot slot')
    args = parser.parse_args()
    output = args.output or args.input.with_name(f'{args.input.stem}_modify.bin')
    try:
        modified = modify_gpt(args.input.read_bytes(), args.slot)
        output.write_bytes(modified)
    except (OSError, ValueError, struct.error) as error:
        parser.error(str(error))
    print(f'Written {len(modified)} bytes: {output}')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
