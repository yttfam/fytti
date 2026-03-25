#!/usr/bin/env python3
"""Generate a minimal test SWF file with shapes and colors."""
import struct, zlib, io

def write_bits(buf, bits, n):
    """Write n bits to a bit buffer (list of (bit, count) tuples)."""
    buf.append((bits, n))

def flush_bits(bit_buf):
    """Convert bit buffer to bytes."""
    result = bytearray()
    current = 0
    pos = 0
    for bits, n in bit_buf:
        for i in range(n - 1, -1, -1):
            current = (current << 1) | ((bits >> i) & 1)
            pos += 1
            if pos == 8:
                result.append(current)
                current = 0
                pos = 0
    if pos > 0:
        current <<= (8 - pos)
        result.append(current)
    return bytes(result)

def rect_bits(x_min, x_max, y_min, y_max):
    """Encode a RECT in SWF bit format (values in twips)."""
    vals = [x_min, x_max, y_min, y_max]
    max_val = max(abs(v) for v in vals)
    nbits = max_val.bit_length() + 1 if max_val > 0 else 1
    buf = []
    write_bits(buf, nbits, 5)
    for v in vals:
        if v < 0:
            v = v & ((1 << nbits) - 1)
        write_bits(buf, v, nbits)
    return flush_bits(buf)

def make_tag(code, data=b''):
    length = len(data)
    if length < 0x3F:
        header = struct.pack('<H', (code << 6) | length)
    else:
        header = struct.pack('<HI', (code << 6) | 0x3F, length)
    return header + data

def make_swf():
    width_twips = 400 * 20   # 400px
    height_twips = 300 * 20  # 300px

    tags = bytearray()

    # SetBackgroundColor (tag 9) — dark blue
    tags += make_tag(9, bytes([26, 26, 46]))

    # DefineShape (tag 2) — red rectangle
    shape_data = bytearray()
    shape_data += struct.pack('<H', 1)  # character ID = 1
    shape_data += rect_bits(0, 200*20, 0, 100*20)  # bounds

    # Fill styles: 1 solid red
    shape_data += bytes([1])  # fill count
    shape_data += bytes([0x00])  # solid fill type
    shape_data += bytes([229, 69, 96])  # RGB red

    # Line styles: 0
    shape_data += bytes([0])

    # Shape records (bit-packed)
    shape_bits = []
    # NumFillBits=1, NumLineBits=0
    write_bits(shape_bits, 1, 4)
    write_bits(shape_bits, 0, 4)

    # StyleChange: MoveTo(0,0), FillStyle0=1
    write_bits(shape_bits, 0, 1)  # non-edge
    write_bits(shape_bits, 0b00011, 5)  # flags: hasMoveTo + hasFillStyle0
    write_bits(shape_bits, 1, 5)  # moveTo nbits
    write_bits(shape_bits, 0, 1)  # x=0
    write_bits(shape_bits, 0, 1)  # y=0
    write_bits(shape_bits, 1, 1)  # fillStyle0 = 1

    # StraightEdge: LineTo(200*20, 0) — right
    write_bits(shape_bits, 1, 1)  # edge
    write_bits(shape_bits, 1, 1)  # straight
    nbits = 13
    write_bits(shape_bits, nbits - 2, 4)  # nbits-2
    write_bits(shape_bits, 0, 1)  # not general
    write_bits(shape_bits, 0, 1)  # horizontal
    dx = 200 * 20
    if dx < 0: dx = dx & ((1 << nbits) - 1)
    write_bits(shape_bits, dx, nbits)

    # StraightEdge: LineTo(0, 100*20) — down
    write_bits(shape_bits, 1, 1)
    write_bits(shape_bits, 1, 1)
    write_bits(shape_bits, nbits - 2, 4)
    write_bits(shape_bits, 0, 1)
    write_bits(shape_bits, 1, 1)  # vertical
    dy = 100 * 20
    if dy < 0: dy = dy & ((1 << nbits) - 1)
    write_bits(shape_bits, dy, nbits)

    # StraightEdge: LineTo(-200*20, 0) — left
    write_bits(shape_bits, 1, 1)
    write_bits(shape_bits, 1, 1)
    write_bits(shape_bits, nbits - 2, 4)
    write_bits(shape_bits, 0, 1)
    write_bits(shape_bits, 0, 1)  # horizontal
    dx_neg = (-200 * 20) & ((1 << nbits) - 1)
    write_bits(shape_bits, dx_neg, nbits)

    # StraightEdge: LineTo(0, -100*20) — up
    write_bits(shape_bits, 1, 1)
    write_bits(shape_bits, 1, 1)
    write_bits(shape_bits, nbits - 2, 4)
    write_bits(shape_bits, 0, 1)
    write_bits(shape_bits, 1, 1)  # vertical
    dy_neg = (-100 * 20) & ((1 << nbits) - 1)
    write_bits(shape_bits, dy_neg, nbits)

    # EndShape
    write_bits(shape_bits, 0, 1)  # non-edge
    write_bits(shape_bits, 0, 5)  # flags=0 = end

    shape_data += flush_bits(shape_bits)
    tags += make_tag(2, bytes(shape_data))

    # DefineShape (tag 2) — green circle (approximated as a shape)
    shape2 = bytearray()
    shape2 += struct.pack('<H', 2)  # ID=2
    shape2 += rect_bits(50*20, 150*20, 120*20, 220*20)  # bounds
    shape2 += bytes([1, 0x00, 46, 204, 113])  # 1 fill, solid green
    shape2 += bytes([0])  # 0 lines

    bits2 = []
    write_bits(bits2, 1, 4)  # nfill bits
    write_bits(bits2, 0, 4)  # nline bits
    # StyleChange: MoveTo(100*20, 120*20), Fill0=1
    write_bits(bits2, 0, 1)
    write_bits(bits2, 0b00011, 5)
    nb = 13
    write_bits(bits2, nb, 5)
    mx = 100 * 20
    write_bits(bits2, mx if mx >= 0 else mx & ((1<<nb)-1), nb)
    my = 120 * 20
    write_bits(bits2, my if my >= 0 else my & ((1<<nb)-1), nb)
    write_bits(bits2, 1, 1)  # fill0=1
    # Line right
    write_bits(bits2, 1, 1); write_bits(bits2, 1, 1)
    write_bits(bits2, nb-2, 4); write_bits(bits2, 0, 1); write_bits(bits2, 0, 1)
    write_bits(bits2, 100*20, nb)
    # Line down
    write_bits(bits2, 1, 1); write_bits(bits2, 1, 1)
    write_bits(bits2, nb-2, 4); write_bits(bits2, 0, 1); write_bits(bits2, 1, 1)
    write_bits(bits2, 100*20, nb)
    # Line left
    write_bits(bits2, 1, 1); write_bits(bits2, 1, 1)
    write_bits(bits2, nb-2, 4); write_bits(bits2, 0, 1); write_bits(bits2, 0, 1)
    write_bits(bits2, (-100*20) & ((1<<nb)-1), nb)
    # Line up
    write_bits(bits2, 1, 1); write_bits(bits2, 1, 1)
    write_bits(bits2, nb-2, 4); write_bits(bits2, 0, 1); write_bits(bits2, 1, 1)
    write_bits(bits2, (-100*20) & ((1<<nb)-1), nb)
    # End
    write_bits(bits2, 0, 1); write_bits(bits2, 0, 5)
    shape2 += flush_bits(bits2)
    tags += make_tag(2, bytes(shape2))

    # PlaceObject (tag 4) — place shape 1 at (50, 50)
    po1 = struct.pack('<HH', 1, 1)  # charID=1, depth=1
    # Matrix: translate only (50*20, 50*20)
    m_bits = []
    write_bits(m_bits, 0, 1)  # no scale
    write_bits(m_bits, 0, 1)  # no rotate
    nb = 13
    write_bits(m_bits, nb, 5)
    tx = 50 * 20
    ty = 50 * 20
    write_bits(m_bits, tx if tx >= 0 else tx & ((1<<nb)-1), nb)
    write_bits(m_bits, ty if ty >= 0 else ty & ((1<<nb)-1), nb)
    po1 += flush_bits(m_bits)
    tags += make_tag(4, bytes(po1))

    # PlaceObject — place shape 2 at (0,0) depth 2
    po2 = struct.pack('<HH', 2, 2)
    m_bits2 = []
    write_bits(m_bits2, 0, 1)
    write_bits(m_bits2, 0, 1)
    write_bits(m_bits2, 1, 5)
    write_bits(m_bits2, 0, 1)
    write_bits(m_bits2, 0, 1)
    po2 += flush_bits(m_bits2)
    tags += make_tag(4, bytes(po2))

    # ShowFrame (tag 1)
    tags += make_tag(1)

    # End (tag 0)
    tags += make_tag(0)

    # Build SWF body (after header)
    body = rect_bits(0, width_twips, 0, height_twips)
    body += struct.pack('<H', 24 * 256)  # frame rate 24fps (8.8 fixed)
    body += struct.pack('<H', 1)  # frame count
    body += bytes(tags)

    # Header
    file_length = 8 + len(body)
    header = b'FWS'
    header += bytes([10])  # version 10
    header += struct.pack('<I', file_length)

    return header + body

swf_data = make_swf()
with open('test.swf', 'wb') as f:
    f.write(swf_data)
print(f"Generated test.swf ({len(swf_data)} bytes)")
