# jsonflatten

Flattens deeply nested JSON into flat dot-path `key=value` rows -
`{"a":{"b":[1,2]}}` becomes `a.b.0=1` / `a.b.1=2` - and unflattens those
rows back into the original structure. Node has `flat`, Python has
`flatten-json`; this is the same idea as a standalone Rust binary instead
of a library import, for piping nested JSON through `grep`/`sort`/`awk`
or diffing two configs line-by-line without a JSON-aware diff tool.

## Usage

```bash
jsonflatten flatten config.json          # dot-path rows to stdout
jsonflatten flatten -                    # read JSON from stdin
jsonflatten unflatten flat.txt           # rebuild JSON from rows
jsonflatten unflatten -                  # read rows from stdin
```

```
$ jsonflatten flatten nested.json
active=true
counts.0=1
counts.1=2
counts.2=3
empty_list=[]
meta.owners.0="alice"
meta.owners.1="bob"
meta.region.code=null
meta.region.country="US"
name="acme-widget"
releases.0.breaking=false
releases.0.tag="v1"
```

Values are JSON-encoded, not raw text, so `a.b=1` (number) and
`a.b="1"` (string) round-trip as different types, and a value
containing a literal `=` (inside a quoted string) parses back correctly
because only the *first* `=` on a line splits key from value.

Object keys and array indices share one `.`-separated path - there's no
`a.b[0]` bracket notation, matching the exact `a.b.0=1` format the tool
is meant to produce. Rows are printed in **sorted-by-string-path**
order, so `a.10` sorts before `a.2` textually; this only affects display
order, not correctness of either direction.

An empty object or array is kept as a leaf at its own path
(`empty_list=[]`) instead of silently vanishing, since dropping it would
make `unflatten(flatten(x))` lossy.

## How unflatten decides array vs. object

A flat path alone can't say whether `a.0` means "index 0 of an array
named `a`" or "key `\"0\"` of an object named `a`" - both produce the
identical path. `unflatten` resolves this per node: if **every** child
key of a node parses as a plain non-negative integer, that node becomes
a JSON array (indices sorted numerically, any gap - which `flatten`'s
own output never actually produces - filled with `null` rather than
panicking); otherwise it becomes a JSON object. This is the same
heuristic the Node `flat` package uses, and it's why `unflatten(flatten(x))`
reproduces arrays-of-objects, arrays-of-arrays, and plain objects all
correctly, but can't tell a real object with only numeric-string keys
apart from an array (a genuine, documented limitation - see below).

## Status: built and verified, including a real round-trip test against realistic fixtures

- **20 unit tests** (`cargo test --lib`): flattening nested objects,
  arrays of scalars, arrays of objects, deeply mixed structures (an
  object inside an array inside an array); empty object/array preserved
  as leaves rather than dropped; `null`/`bool`/string values surviving
  unchanged; a top-level bare scalar using the empty-string sentinel
  path; sparse-array gap-filling with `null`; the `key=value` line
  parser/renderer round-tripping a value that itself contains a literal
  `=`; and rejecting a line with no `=` or an invalid-JSON value with a
  specific error rather than panicking.
- **10 of those are direct `unflatten(flatten(x)) == x` round-trip
  tests** at the `serde_json::Value` level: a 3-level nested object, an
  array of scalars, an array of objects with mixed field types, a
  structure mixing objects-in-arrays-in-objects with a `null` field, two
  different empty-container placements, a top-level scalar, a top-level
  array, and negative/float numbers.
- **Live-verified end-to-end through the actual CLI, not just the
  library**: built a realistic nested fixture (a version/metadata
  object, an array of role-tagged owner strings, a nested
  present-vs-null field pair, an array of objects each with a different
  field set including one with an embedded empty object, an empty
  top-level array, and a plain scalar array) written to a real file,
  ran `jsonflatten flatten` on it, confirmed the flat output was
  exactly the expected key/value set by eye, piped that output through
  `jsonflatten unflatten`, and diffed the result against the original
  file with an independent Python `json.load` equality check (not this
  tool's own parser) - **`EQUAL`**, byte-for-byte structural match.

**Not done / deliberately deferred**: a JSON object whose keys
are *all* plain non-negative integers (`{"0": "x", "1": "y"}`) is
indistinguishable from an array with the same values once flattened,
and `unflatten` will reconstruct it as an array, not the original
object - there is no encoded type tag to disambiguate, the same
limitation the `flat` npm package has. A JSON object containing a
literal empty-string key (`{"": 5}`) collides with the top-level-scalar
sentinel path and isn't reconstructed correctly either - both are
edge cases judged rare enough not to justify an escaping scheme that
would make every normal path uglier. A key containing a literal `.`
isn't supported (the separator is assumed not to appear in real keys,
same assumption most flatten tools make); piping such a key through
this tool produces a wrong split rather than an error.
