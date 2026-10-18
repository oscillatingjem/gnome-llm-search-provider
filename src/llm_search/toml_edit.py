"""Change single values in a TOML file without disturbing anything else.

General TOML writers drop comments, and agents.toml is documented by its comments, so the
settings window edits it line by line instead:

  - a key that exists is changed in place, keeping any comment after the value;
  - a missing key is added right below its table's header (below `id` in [[app]] entries);
  - a missing table is added at the end of the file.

Only single-line scalar values (strings, numbers, booleans) are written. Every edit is checked
by parsing the result and reading the value back; if that fails, nothing is changed.
"""

import json
import re
import tomllib

HEADER = re.compile(r"^\s*(\[\[?)\s*([A-Za-z0-9_.\"' -]+?)\s*(\]\]?)\s*(#.*)?$")


class EditError(ValueError):
    pass


def format_value(value) -> str:
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, (int, float)):
        return repr(value)
    if isinstance(value, str):
        return json.dumps(value)        # a JSON string is a valid TOML basic string
    raise EditError(f"can't write {type(value).__name__} values")


def _headers(lines: list[str]) -> list[tuple[int, str, bool]]:
    """(line index, table name, is array-of-tables) for every table header."""
    out = []
    for i, line in enumerate(lines):
        m = HEADER.match(line)
        if m and (m.group(1) == "[[") == (m.group(3) == "]]"):
            out.append((i, m.group(2).strip(), m.group(1) == "[["))
    return out


def _span(lines: list[str], start: int) -> int:
    """End (exclusive) of the table whose header is at `start`."""
    for i, _name, _arr in _headers(lines):
        if i > start:
            return i
    return len(lines)


def _key_line(key: str) -> re.Pattern:
    return re.compile(rf"^(\s*){re.escape(key)}(\s*=\s*)(.*)$")


def _split_value(rest: str) -> tuple[str, str] | None:
    """Split 'value  # comment' into (value, comment), respecting '#' inside strings."""
    cuts = [i for i, ch in enumerate(rest) if ch == "#"] + [len(rest)]
    for cut in cuts:
        candidate = rest[:cut].rstrip()
        if candidate and _is_complete(candidate):
            return candidate, rest[len(candidate):]
    return None                          # e.g. an array that continues on the next lines


def _set_in_span(lines: list[str], header: int, end: int, key: str, value,
                 insert_after: int | None = None) -> list[str]:
    pattern = _key_line(key)
    for i in range(header + 1, end):
        m = pattern.match(lines[i])
        if m:
            split = _split_value(m.group(3))
            if split is None:
                raise EditError(f"{key} is a multi-line value; edit it in the file")
            lines[i] = f"{m.group(1)}{key}{m.group(2)}{format_value(value)}{split[1]}"
            return lines
    lines.insert((header if insert_after is None else insert_after) + 1, f"{key} = {format_value(value)}")
    return lines


def _is_complete(value: str) -> bool:
    try:
        tomllib.loads(f"x = {value}")
        return True
    except tomllib.TOMLDecodeError:
        return False


def set_table_value(text: str, table: str, key: str, value) -> str:
    """Set `key` in [table] (e.g. table="search")."""
    lines = text.splitlines()
    for i, name, is_array in _headers(lines):
        if name == table and not is_array:
            return "\n".join(_set_in_span(lines, i, _span(lines, i), key, value)) + "\n"
    tail = ["", f"[{table}]  # added by the settings window", f"{key} = {format_value(value)}"]
    return "\n".join([*lines, *tail]) + "\n"


def set_array_item_value(text: str, array: str, item_id: str, key: str, value) -> str:
    """Set `key` in the [[array]] entry whose id = item_id (e.g. the [[app]] with id "claude-cli")."""
    lines = text.splitlines()
    id_line = _key_line("id")
    for i, name, is_array in _headers(lines):
        if name != array or not is_array:
            continue
        end = _span(lines, i)
        for j in range(i + 1, end):
            m = id_line.match(lines[j])
            split = m and _split_value(m.group(3))
            if split and tomllib.loads(f"x = {split[0]}")["x"] == item_id:
                return "\n".join(_set_in_span(lines, i, end, key, value, insert_after=j)) + "\n"
    raise EditError(f"no [[{array}]] with id {item_id!r}")


def apply(text: str, edits: list[tuple[str, str, str | None, object]]) -> str:
    """Apply edits and verify them.

    Each edit is (table, key, item_id, value): item_id None means a [table]; otherwise the
    [[table]] entry with that id.
    """
    for table, key, item_id, value in edits:
        if item_id is None:
            text = set_table_value(text, table, key, value)
        else:
            text = set_array_item_value(text, table, item_id, key, value)
    try:
        data = tomllib.loads(text)
    except tomllib.TOMLDecodeError as e:
        raise EditError(f"the edit would break the file: {e}") from e
    for table, key, item_id, value in edits:
        if item_id is None:
            got = data.get(table, {}).get(key)
        else:
            got = next((x.get(key) for x in data.get(table, []) if x.get("id") == item_id), None)
        if got != value:
            raise EditError(f"{table}.{key} reads back as {got!r}, not {value!r}")
    return text
