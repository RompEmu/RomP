"""Writes tiny programs that turn the screen on with a colour, one per system, for smoke-testing
Romp's cores. They're written here, so there's nothing to license."""

import struct
import sys
from pathlib import Path


def nes():
    code = bytes([
        0x78, 0xD8,                    # sei, cld
        0x2C, 0x02, 0x20, 0x10, 0xFB,  # wait for the PPU to warm up, twice
        0x2C, 0x02, 0x20, 0x10, 0xFB,
        0xA9, 0x3F, 0x8D, 0x06, 0x20,  # point the PPU at the backdrop colour
        0xA9, 0x00, 0x8D, 0x06, 0x20,
        0xA9, 0x21, 0x8D, 0x07, 0x20,  # light blue
        0xA9, 0x00, 0x8D, 0x06, 0x20,  # and away from the palette again
        0x8D, 0x06, 0x20,
        0xA9, 0x08, 0x8D, 0x01, 0x20,  # show the background
        0x4C, 0x28, 0xC0,              # jmp $c028, forever
        0x40,                          # rti at $c02b, for interrupts
    ])
    prg = bytearray(code.ljust(16 * 1024, b"\xff"))
    prg[-6:] = bytes([0x2B, 0xC0, 0x00, 0xC0, 0x2B, 0xC0])  # NMI, reset and IRQ vectors
    return b"NES\x1a" + bytes([1, 0]) + bytes(10) + prg


def atari2600():
    code = bytes([
        0x78, 0xD8,                    # sei, cld
        0xA9, 0x9A, 0x85, 0x09,        # light blue background (COLUBK)
        0xA9, 0x00, 0x85, 0x01,        # vertical blank off
        0xA9, 0x02, 0x85, 0x00,        # frame: three lines of vertical sync
        0x85, 0x02, 0x85, 0x02, 0x85, 0x02,
        0xA9, 0x00, 0x85, 0x00,
        0xA2, 0x00, 0x85, 0x02, 0xCA, 0xD0, 0xFB,  # then 256 lines
        0xA2, 0x03, 0x85, 0x02, 0xCA, 0xD0, 0xFB,  # and 3 more
        0x4C, 0x0A, 0xF0,              # next frame
    ])
    rom = bytearray(code.ljust(4096, b"\xff"))
    rom[0xFFC:0x1000] = bytes([0x00, 0xF0, 0x00, 0xF0])  # reset and IRQ vectors: $f000
    return bytes(rom)


def c64():
    # 10 SYS2061, then light blue border and background, forever.
    basic = bytes([0x0B, 0x08, 0x0A, 0x00, 0x9E]) + b"2061" + bytes([0x00, 0x00, 0x00])
    code = bytes([0xA9, 0x0E, 0x8D, 0x20, 0xD0, 0x8D, 0x21, 0xD0, 0x4C, 0x15, 0x08])
    return bytes([0x01, 0x08]) + basic + code


def dos():
    return bytes([
        0xB8, 0x13, 0x00, 0xCD, 0x10,  # 320x200 in 256 colours
        0xB8, 0x00, 0xA0, 0x8E, 0xC0,  # es = video memory
        0x31, 0xFF, 0xB0, 0x09,        # di = 0, al = light blue
        0xB9, 0x00, 0xFA, 0xF3, 0xAA,  # fill the screen
        0xEB, 0xFE,                    # forever
    ])


def gameboy():
    rom = bytearray(32 * 1024)
    rom[0x100:0x104] = bytes([0x00, 0xC3, 0x50, 0x01])  # nop, jp $0150
    rom[0x104:0x108] = bytes([0xCE, 0xED, 0x66, 0x66])  # what mGBA looks for to recognise a cartridge
    rom[0x134:0x13D] = b"ROMPSMOKE"
    rom[0x150:0x15B] = bytes([
        0xF3,                          # di
        0x3E, 0xE4, 0xE0, 0x47,        # background palette, colour 0 white
        0x3E, 0x91, 0xE0, 0x40,        # screen and background on
        0x18, 0xFE,                    # forever
    ])
    rom[0x14D] = (-sum(rom[0x134:0x14D]) - 25) & 0xFF  # header checksum
    return bytes(rom)


def gba():
    words = [
        0xEA00002E,  # b $c0, past the header
    ]
    code = [
        0xE3A00301,  # mov r0, #0x04000000
        0xE3A01000,  # mov r1, #0
        0xE1C010B0,  # strh r1, [r0]       ; mode 0, nothing but the backdrop
        0xE3A00405,  # mov r0, #0x05000000
        0xE3A01C7E,  # mov r1, #0x7e00     ; light blue
        0xE1C010B0,  # strh r1, [r0]       ; backdrop colour
        0xEAFFFFFE,  # b .                 ; forever
    ]
    rom = bytearray(0x200)
    rom[0:4] = struct.pack("<I", words[0])
    rom[0xA0:0xAC] = b"ROMPSMOKE".ljust(12, b"\0")
    rom[0xB2] = 0x96
    rom[0xBD] = (-sum(rom[0xA0:0xBD]) - 0x19) & 0xFF  # header checksum
    for i, word in enumerate(code):
        rom[0xC0 + 4 * i:0xC4 + 4 * i] = struct.pack("<I", word)
    return bytes(rom)


def snes():
    code = bytes([
        0x78,                          # sei
        0xA9, 0x8F, 0x8D, 0x00, 0x21,  # screen off while setting up
        0x9C, 0x21, 0x21,              # colour 0
        0xA9, 0x00, 0x8D, 0x22, 0x21,  # light blue, low byte
        0xA9, 0x7E, 0x8D, 0x22, 0x21,  # and high byte
        0xA9, 0x0F, 0x8D, 0x00, 0x21,  # screen on, full brightness
        0x80, 0xFE,                    # forever
        0x40,                          # rti at $801a, for interrupts
    ])
    rom = bytearray(code.ljust(32 * 1024, b"\xff"))
    rom[0x7FC0:0x7FD5] = b"ROMP SMOKE".ljust(21)
    rom[0x7FD5] = 0x20  # LoROM
    rom[0x7FD6] = 0x00  # ROM only
    rom[0x7FD7] = 0x05  # 32 KB
    rom[0x7FD8] = 0x00
    rom[0x7FD9] = 0x01  # North America
    rom[0x7FDA:0x7FDC] = bytes([0x00, 0x00])
    for at in range(0x7FE4, 0x7FF0, 2):
        rom[at:at + 2] = bytes([0x1A, 0x80])  # native mode vectors
    for at in range(0x7FF4, 0x8000, 2):
        rom[at:at + 2] = bytes([0x1A, 0x80])  # emulation mode vectors
    rom[0x7FFC:0x7FFE] = bytes([0x00, 0x80])  # reset: $8000
    rom[0x7FDC:0x7FE0] = bytes([0xFF, 0xFF, 0x00, 0x00])
    checksum = sum(rom) & 0xFFFF
    rom[0x7FDC:0x7FE0] = struct.pack("<HH", checksum ^ 0xFFFF, checksum)
    return bytes(rom)


def genesis():
    rom = bytearray(0x4000)
    rom[0:8] = struct.pack(">II", 0x00FFFE00, 0x200)  # stack and entry point
    rom[0x100:0x110] = b"SEGA MEGA DRIVE "
    rom[0x120:0x130] = b"ROMP SMOKE".ljust(16)
    rom[0x150:0x160] = b"ROMP SMOKE".ljust(16)
    rom[0x1A0:0x1A8] = struct.pack(">II", 0, len(rom) - 1)
    rom[0x1A8:0x1B0] = struct.pack(">II", 0xFF0000, 0xFFFFFF)
    rom[0x1F0:0x1F3] = b"JUE"
    control, data = 0x00C00004, 0x00C00000

    def word(value, port):
        return struct.pack(">HHI", 0x33FC, value, port)

    code = (
        word(0x8004, control)             # mode register 1
        + word(0x8144, control)           # display on, Mega Drive mode
        + word(0x8700, control)           # backdrop: palette 0, colour 0
        + struct.pack(">HII", 0x23FC, 0xC0000000, control)  # write colour 0
        + word(0x0EA4, data)              # light blue
        + bytes([0x60, 0xFE])             # forever
    )
    rom[0x200:0x200 + len(code)] = code
    return bytes(rom)


def master_system():
    code = bytes([
        0xF3,                                      # di
        0x31, 0xF0, 0xDF,                          # ld sp, $dff0
        0x3E, 0x04, 0xD3, 0xBF, 0x3E, 0x80, 0xD3, 0xBF,  # mode 4
        0x3E, 0x40, 0xD3, 0xBF, 0x3E, 0x81, 0xD3, 0xBF,  # display on
        0x3E, 0x00, 0xD3, 0xBF, 0x3E, 0xC0, 0xD3, 0xBF,  # background colour 0, which empty tiles show
        0x3E, 0x3C, 0xD3, 0xBE,                    # light blue
        0x18, 0xFE,                                # forever
    ])
    rom = bytearray(code.ljust(32 * 1024, b"\xff"))
    rom[0x7FF0:0x7FF8] = b"TMR SEGA"
    rom[0x7FFF] = 0x4C  # export Master System, 32 KB
    return bytes(rom)


def pc_engine():
    code = bytes([
        0x78, 0xD8,                    # sei, cld
        0xA9, 0xFF, 0x53, 0x01,        # hardware page at $0000
        0x9C, 0x02, 0x04, 0x9C, 0x03, 0x04,  # colour 0
        0xA9, 0x4F, 0x8D, 0x04, 0x04,  # light blue: green 5, red 1, blue 7
        0xA9, 0x01, 0x8D, 0x05, 0x04,
        0x03, 0x05, 0x13, 0x80, 0x23, 0x00,  # display on (background)
        0x80, 0xFE,                    # forever
        0x40,                          # rti at $e01e, for interrupts
    ])
    rom = bytearray(code.ljust(8 * 1024, b"\xff"))
    rom[0x1FF6:0x2000] = bytes([0x1E, 0xE0] * 4 + [0x00, 0xE0])  # interrupts, then reset: $e000
    return bytes(rom)


def zx_spectrum():
    code = bytes([
        0xF3,                    # di
        0x3E, 0x01, 0xD3, 0xFE,  # blue border
        0x21, 0x00, 0x58,        # ld hl, attributes
        0x11, 0x01, 0x58,        # ld de, attributes + 1
        0x01, 0xFF, 0x02,        # ld bc, 767
        0x36, 0x28,              # ld (hl), cyan paper
        0xED, 0xB0,              # ldir
        0x18, 0xFE,              # forever
    ])
    ram = bytearray(48 * 1024)
    ram[0x8000 - 0x4000:0x8000 - 0x4000 + len(code)] = code
    # A version 1 snapshot: registers, then the 48 KB of memory, uncompressed.
    header = bytearray(30)
    header[6:8] = struct.pack("<H", 0x8000)   # PC
    header[8:10] = struct.pack("<H", 0xFF00)  # SP
    header[12] = 0x02                         # blue border, not compressed
    header[27] = 0                            # interrupts off
    header[29] = 1                            # interrupt mode 1
    return bytes(header + ram)


def nintendo_ds():
    arm9 = [
        0xE3A00301,  # mov r0, #0x04000000
        0xE2800C03,  # add r0, r0, #0x300
        0xE2800004,  # add r0, r0, #4      ; POWCNT1
        0xE3A01902,  # mov r1, #0x8000
        0xE3811003,  # orr r1, r1, #3      ; screens and 2D engine A on, A on top
        0xE1C010B0,  # strh r1, [r0]
        0xE3A00301,  # mov r0, #0x04000000 ; DISPCNT
        0xE3A01801,  # mov r1, #0x10000    ; graphics mode, nothing but the backdrop
        0xE5801000,  # str r1, [r0]
        0xE3A00405,  # mov r0, #0x05000000
        0xE3A01C7E,  # mov r1, #0x7e00     ; light blue
        0xE1C010B0,  # strh r1, [r0]       ; backdrop colour
        0xEAFFFFFE,  # b .                 ; forever
    ]
    arm7 = [0xEAFFFFFE]  # b ., the ARM7 just waits
    rom = bytearray(0x1000)
    rom[0:12] = b"ROMP SMOKE".ljust(12, b"\0")
    rom[12:16] = b"####"
    rom[0x20:0x30] = struct.pack("<IIII", 0x200, 0x02000000, 0x02000000, 4 * len(arm9))
    rom[0x30:0x40] = struct.pack("<IIII", 0x300, 0x02380000, 0x02380000, 4 * len(arm7))
    rom[0x80:0x88] = struct.pack("<II", len(rom), 0x4000)
    rom[0x15C:0x15E] = struct.pack("<H", 0xCF56)
    rom[0x15E:0x160] = struct.pack("<H", crc16(rom[:0x15E]))
    for i, word in enumerate(arm9):
        rom[0x200 + 4 * i:0x204 + 4 * i] = struct.pack("<I", word)
    rom[0x300:0x304] = struct.pack("<I", arm7[0])
    return bytes(rom)


def crc16(data):
    crc = 0xFFFF
    for byte in data:
        crc ^= byte
        for _ in range(8):
            crc = (crc >> 1) ^ 0xA001 if crc & 1 else crc >> 1
    return crc


GAMES = {
    "nes.nes": nes,
    "atari2600.a26": atari2600,
    "c64.prg": c64,
    "dos.com": dos,
    "gameboy.gb": gameboy,
    "gba.gba": gba,
    "snes.sfc": snes,
    "genesis.md": genesis,
    "mastersystem.sms": master_system,
    "pcengine.pce": pc_engine,
    "zxspectrum.z80": zx_spectrum,
    "nintendods.nds": nintendo_ds,
}

if __name__ == "__main__":
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    for name, make in GAMES.items():
        (out / name).write_bytes(make())
