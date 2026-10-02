"""Writes a tiny NES program that fills the screen with light blue, for smoke-testing Romp."""

import sys

program = bytes(
    [
        0x78,              # sei
        0xD8,              # cld
        0x2C, 0x02, 0x20,  # bit $2002   ; wait for the PPU to warm up
        0x10, 0xFB,        # bpl -5
        0x2C, 0x02, 0x20,  # bit $2002
        0x10, 0xFB,        # bpl -5
        0xA9, 0x3F,        # lda #$3f    ; point the PPU at the backdrop colour
        0x8D, 0x06, 0x20,  # sta $2006
        0xA9, 0x00,        # lda #$00
        0x8D, 0x06, 0x20,  # sta $2006
        0xA9, 0x21,        # lda #$21    ; light blue
        0x8D, 0x07, 0x20,  # sta $2007
        0xA9, 0x00,        # lda #$00    ; and away from the palette again
        0x8D, 0x06, 0x20,  # sta $2006
        0x8D, 0x06, 0x20,  # sta $2006
        0xA9, 0x08,        # lda #$08    ; show the background
        0x8D, 0x01, 0x20,  # sta $2001
        0x4C, 0x28, 0xC0,  # jmp $c028   ; forever
        0x40,              # rti         ; at $c02b, for interrupts
    ]
)

PRG_SIZE = 16 * 1024
prg = bytearray(program.ljust(PRG_SIZE, b"\xff"))
# Vectors at $fffa: NMI, reset and IRQ, with the code at $c000.
prg[-6:] = bytes([0x2B, 0xC0, 0x00, 0xC0, 0x2B, 0xC0])
header = b"NES\x1a" + bytes([1, 0]) + bytes(10)

with open(sys.argv[1], "wb") as rom:
    rom.write(header + prg)
