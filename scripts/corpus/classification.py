"""Independent, bounded ICD reader for the pinned digital HCA input (product 165).

This is fixture verification, not a general Level III importer. The Rust production
decoder is checked against this stdlib struct/bz2/zlib interpretation.
"""
import bz2
from collections import Counter
import datetime as dt
import re
import struct
import zlib

LIMIT = 2_000_000
HEADING = re.compile(rb'(?:SDUS|NXUS|NOUS)[^\r\n]{1,70}\r\r\n[^\r\n]{1,70}\r\r\n')


def strip_heading(data):
    match = HEADING.search(data[:256])
    return data[match.end():] if match else data


def source_clock(day, second):
    if not 1 <= day <= 32767 or not 0 <= second < 86400:
        raise ValueError('Invalid HCA source clock')
    return (dt.datetime(1970, 1, 1, tzinfo=dt.timezone.utc)
            + dt.timedelta(days=day - 1, seconds=second)).strftime('%Y-%m-%dT%H:%M:%SZ')


def inspect_hca(raw):
    try:
        return _inspect_hca(raw)
    except (struct.error, EOFError, OSError, zlib.error) as error:
        raise ValueError('Unreadable or truncated HCA container') from error


def _inspect_hca(raw):
    data = strip_heading(raw)
    if data.startswith(b'\x78'):
        parts, size = [], 0
        while data.startswith(b'\x78'):
            decoder = zlib.decompressobj()
            part = decoder.decompress(data, LIMIT + 1 - size)
            size += len(part)
            if size > LIMIT or not decoder.eof:
                raise ValueError('HCA zlib stream exceeds the bound or is truncated')
            parts.append(part)
            data = decoder.unused_data
        data = strip_heading(b''.join(parts))
    if len(data) < 120 or struct.unpack_from('>h', data, 18)[0] != -1:
        raise ValueError('Missing HCA product description')
    code = struct.unpack_from('>h', data, 30)[0]
    if code != 165:
        raise ValueError('Expected digital hydrometeor classification (165)')
    lat, lon = struct.unpack_from('>ii', data, 20)
    start = source_clock(*struct.unpack_from('>hI', data, 40))
    generation = source_clock(*struct.unpack_from('>hI', data, 46))
    elevation = struct.unpack_from('>h', data, 58)[0] / 10
    sym_offset = struct.unpack_from('>I', data, 108)[0] * 2
    if not 120 <= sym_offset < len(data):
        raise ValueError('HCA symbology offset is unavailable')
    body = data[sym_offset:]
    if body.startswith(b'BZh'):
        decoder = bz2.BZ2Decompressor()
        body = decoder.decompress(body, max_length=LIMIT + 1)
        if len(body) > LIMIT or not decoder.eof:
            raise ValueError('HCA bzip2 stream exceeds the bound or is truncated')
    if len(body) < 30:
        raise ValueError('Truncated HCA symbology')
    divider, block, length, layers = struct.unpack_from('>hhIh', body)
    layer_divider, layer_length = struct.unpack_from('>hI', body, 10)
    if (divider, block, layers, layer_divider) != (-1, 1, 1, -1) or length != len(body) or layer_length != len(body) - 16:
        raise ValueError('Unexpected HCA block/layer layout')
    packet, first, bins, _x, _y, _scale, radials = struct.unpack_from('>HHHhhhH', body, 16)
    if packet != 16 or not 0 < bins <= 2000 or not 0 < radials <= 720:
        raise ValueError('Unexpected HCA digital radial geometry')
    cursor, counts = 30, Counter()
    for _ in range(radials):
        if cursor + 6 > len(body):
            raise ValueError('Truncated HCA radial header')
        nbytes, angle, width = struct.unpack_from('>Hhh', body, cursor)
        cursor += 6
        if nbytes != bins or cursor + nbytes > len(body) or not 0 <= angle < 3600 or not 0 < width <= 20:
            raise ValueError('Invalid HCA radial coverage')
        counts.update(body[cursor:cursor + nbytes])
        cursor += nbytes
    if cursor != len(body):
        raise ValueError('Unexpected trailing HCA packet data')
    return {'product_code': code, 'lat': lat / 1000, 'lon': lon / 1000,
            'acquisition_time': start, 'generation_time': generation,
            'elevation_deg': elevation, 'first_bin': first,
            'radials': radials, 'bins': bins,
            'class_counts': {str(k): v for k, v in sorted(counts.items())}}
