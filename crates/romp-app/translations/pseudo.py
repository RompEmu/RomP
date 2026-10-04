"""Writes the pseudo language `xx` from the templates: every string accented, padded and
bracketed, so text that's cut off or left untranslated stands out."""

import pathlib
import re
import sys

ACCENTS = str.maketrans("aeiouAEOU", "áéíöüÅÉÖÜ")
PLACEHOLDER = re.compile(r"(\{[^}]*\}|\\.)")


def pseudo(text):
    parts = PLACEHOLDER.split(text)
    body = "".join(p if PLACEHOLDER.fullmatch(p) else p.translate(ACCENTS) for p in parts)
    return "⟦" + body + "~" * max(1, len(text) * 2 // 5) + "⟧"


def entries(lines):
    """Yields each entry as a list of (keyword, text) with continuation lines joined."""
    entry, field = [], None
    for line in lines:
        if not line.strip():
            if entry:
                yield entry
            entry, field = [], None
        elif line.startswith('"') and field is not None:
            entry[-1] = (entry[-1][0], entry[-1][1] + line[1:-1])
        elif line.startswith("#"):
            entry.append((line, None))
            field = None
        else:
            keyword, _, rest = line.partition(" ")
            entry.append((keyword, rest[1:-1]))
            field = keyword
    if entry:
        yield entry


def convert(pot, po):
    out = []
    for entry in entries(pot.read_text(encoding="utf-8").splitlines()):
        fields = {k: v for k, v in entry if v is not None}
        if fields.get("msgid") == "":
            out.append('msgid ""\nmsgstr ""\n"Content-Type: text/plain; charset=UTF-8\\n"\n'
                       '"Language: xx\\n"\n"Plural-Forms: nplurals=2; plural=(n != 1);\\n"')
            continue
        lines = [k for k, v in entry if v is None]
        for key in ("msgctxt", "msgid", "msgid_plural"):
            if key in fields:
                lines.append(f'{key} "{fields[key]}"')
        if "msgid_plural" in fields:
            lines.append(f'msgstr[0] "{pseudo(fields["msgid"])}"')
            lines.append(f'msgstr[1] "{pseudo(fields["msgid_plural"])}"')
        else:
            lines.append(f'msgstr "{pseudo(fields["msgid"])}"')
        out.append("\n".join(lines))
    po.parent.mkdir(parents=True, exist_ok=True)
    po.write_text("\n\n".join(out) + "\n", encoding="utf-8")


root = pathlib.Path(sys.argv[1])
for name in ("romp-app", "rust"):
    convert(root / f"{name}.pot", root / "xx" / "LC_MESSAGES" / f"{name}.po")
