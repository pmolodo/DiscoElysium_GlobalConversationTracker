#!/usr/bin/env python

"""Read Disco Elysium save data in the ".ntwtf.lua" file.

Reads from PixelCrusher.DialogSystem's "raw data" binary blob into Python data
and emits JSON.

Port of the C# ApplyRawData / ReadValue / ReadTable routines, which use a .NET
BinaryReader. Relevant .NET BinaryReader semantics reproduced here:

  - Int32/Double are little-endian.
  - ReadString is prefixed with its byte length, encoded 7 bits at a time
    (LEB128-style), followed by the bytes in the reader encoding (UTF-8 default).
  - ReadChar (default UTF-8) reads a single byte for the ASCII type-code markers
    used by this format ('T', 'S', 'N', 'B', 'X').

A Lua table has an array part (List) and a hash part (Dict); the C# LuaTable
keeps them separate. This module combines them into a single dict, using
1-indexed indices as the keys for the list part. ie, if we have:

    list_part = ["a", "b"]
    dict_part = [0: "c", 10: "d"]

Then we produce:

    {0: "c", 1: "a", 2: "b", 10: "d"}

"""

import argparse
import json
import struct
import sys
import traceback

# Type-code markers (as produced by reader.ReadChar / PeekChar).
TABLE = ord("T")  # 84 - a nested table follows
STRING = "S"
NUMBER = "N"
BOOLEAN = "B"
NIL = "X"


###############################################################################
# Core functions
###############################################################################


class BinaryReader:
    """Minimal little-endian reader mirroring the .NET BinaryReader methods used."""

    def __init__(self, data):
        self.data = data
        self.pos = 0

    def read_byte(self):
        b = self.data[self.pos]
        self.pos += 1
        return b

    def peek_byte(self):
        # Mirrors PeekChar: returns -1 at end of stream, does not advance.
        if self.pos >= len(self.data):
            return -1
        return self.data[self.pos]

    def read_int32(self):
        (value,) = struct.unpack_from("<i", self.data, self.pos)
        self.pos += 4
        return value

    def read_double(self):
        (value,) = struct.unpack_from("<d", self.data, self.pos)
        self.pos += 8
        return value

    def read_boolean(self):
        return self.read_byte() != 0

    def read_char(self):
        # Type-code markers are ASCII, so a single byte suffices.
        return chr(self.read_byte())

    def read_7bit_encoded_int(self):
        # .NET string-length prefix: base-128, low 7 bits per byte, high bit = continue.
        count = 0
        shift = 0
        while True:
            b = self.read_byte()
            count |= (b & 0x7F) << shift
            if (b & 0x80) == 0:
                break
            shift += 7
        return count

    def read_string(self):
        length = self.read_7bit_encoded_int()
        value = self.data[self.pos : self.pos + length].decode("utf-8")
        self.pos += length
        return value


def read_value(reader):
    if reader.peek_byte() == TABLE:
        return read_table(reader)

    c = reader.read_char()
    if c == STRING:
        return reader.read_string()
    if c == NUMBER:
        value = reader.read_double()
        # Collapse integer-valued doubles to Python ints. is_integer() is False
        # for inf/nan, so those stay floats and never reach int().
        if value.is_integer():
            return int(value)
        return value
    if c == BOOLEAN:
        return reader.read_boolean()
    if c == NIL:
        return None
    raise ValueError(f"ReadValue unhandled type code {c!r}")


def read_table(reader):
    reader.read_byte()  # consume the 'T' table marker (C#: reader.Read())
    combined = {}

    list_count = reader.read_int32()
    for i in range(1, list_count + 1):
        # lua lists by convention are 1-indexed
        value = read_value(reader)
        combined[i] = value

    dict_count = reader.read_int32()
    for _ in range(dict_count):
        key = read_value(reader)
        value = read_value(reader)
        if key in combined:
            raise ValueError("Duplicate key found between list and dict parts")
        combined[key] = value
    return combined


def apply_raw_data(data):
    """Read the five top-level tables from the blob, returning a dict of them."""
    reader = BinaryReader(data)
    result = {}
    for name in ("Actor", "Item", "Location", "Variable", "Conversation"):
        result[name] = read_table(reader)
    return result


def read_raw_data(input_path):
    with open(input_path, "rb") as f:
        data = f.read()
    return apply_raw_data(data)


def read_and_output_raw_data(input_path, output_path=None, indent=2):
    """Read the raw binary data from input_path, convert to Python data, and write JSON to output_path or stdout."""
    result = read_raw_data(input_path)
    text = json.dumps(result, indent=indent, ensure_ascii=False)
    if output_path:
        with open(output_path, "w", encoding="utf-8") as f:
            f.write(text)
    else:
        print(text)


###############################################################################
# CLI
###############################################################################


def get_parser():
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    parser.add_argument("input", help="Path to the raw binary data file")
    parser.add_argument("-o", "--output", help="Output JSON path (default: stdout)")
    parser.add_argument("--indent", type=int, default=2, help="JSON indentation width")
    return parser


def main(argv=None):
    if argv is None:
        argv = sys.argv[1:]
    parser = get_parser()
    args = parser.parse_args(argv)
    try:
        read_and_output_raw_data(args.input, args.output, args.indent)
    except Exception:  # pylint: disable=broad-except
        traceback.print_exc()
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
