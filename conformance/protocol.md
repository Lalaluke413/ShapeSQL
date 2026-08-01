# ShapeSQL Conformance Adapter Protocol

## 1. Status

This document defines adapter protocol version `0.1`. It is non-normative for
ShapeSQL language semantics and normative for a process claiming compatibility
with this protocol version.

The protocol lets a conformance runner test an implementation without
standardizing that implementation's production API. The adapter may translate
requests into any catalog, logical representation, physical plan, simulator,
device buffer, or execution system. Shape IR is not part of this boundary.

## 2. Transport

The runner starts one adapter process and communicates over its standard
streams.

- Standard input and standard output use UTF-8 JSON Lines.
- Each line contains exactly one complete JSON object followed by LF.
- A JSON object MUST NOT contain an unescaped line break.
- Duplicate object member names and unknown semantic members are invalid.
- Standard output is reserved exclusively for protocol responses.
- Human-readable diagnostics may be written to standard error and are not
  interpreted by the protocol.

The process receives requests sequentially. The runner sends one request and
waits for its response before sending the next; version `0.1` has no
pipelining or concurrency. The adapter returns exactly one response for each
request, in request order.

After the runner closes standard input, the adapter MUST finish any current
response and exit successfully. A greeting, log line, blank line, extra
response, premature exit, or nonzero exit is a protocol failure.

Command parsing, process environment, current directory, timeout duration,
output capture limits, and operating-system sandboxing are runner concerns and
are not protocol messages.

## 3. Request

A request has this shape:

```json
{
  "protocol_version": "0.1",
  "case_id": "evaluation-left-join-null-extension",
  "source": {
    "encoding": "utf-8",
    "contents": "SELECT l.id, r.id FROM left_rows AS l LEFT JOIN right_rows AS r ON l.id = r.id;\n"
  },
  "catalog": {
    "format_version": "0.1",
    "relations": []
  },
  "snapshot": {
    "format_version": "0.1",
    "relations": []
  }
}
```

`case_id` is copied exactly from the manifest. `catalog` and `snapshot` are
the validated fixture documents embedded without semantic transformation.
The expected outcome is never sent to the adapter.

### 3.1 Source bytes

The `source` object preserves the exact bytes of the case source file:

- `encoding: "utf-8"` means `contents` is the decoded source text. Re-encoding
  it as UTF-8 reconstructs the original bytes exactly.
- `encoding: "base64"` means `contents` is the canonical RFC 4648 base64
  encoding, with required `=` padding and no whitespace, of the original
  source bytes.

A runner MUST use `utf-8` when the source bytes are valid UTF-8 and `base64`
otherwise. The second form permits end-to-end tests of the ShapeSQL lexical
rule that rejects invalid UTF-8. An adapter MUST support both forms.

The base64 representation is transport encoding only. The candidate front end
receives the decoded bytes and determines their ShapeSQL lexical validity.

## 4. Response

A response repeats the protocol version and case identifier and contains one
observed outcome:

```json
{
  "protocol_version": "0.1",
  "case_id": "evaluation-left-join-null-extension",
  "outcome": {
    "kind": "success",
    "collection": "bag",
    "schema": [
      {
        "name": "id",
        "type": {
          "scalar": "int64",
          "nullable": false
        }
      }
    ],
    "rows": [
      ["1"]
    ]
  }
}
```

An error response is:

```json
{
  "protocol_version": "0.1",
  "case_id": "evaluation-division-by-zero",
  "outcome": {
    "kind": "error",
    "phase": "evaluation"
  }
}
```

Successful schemas, collection kinds, rows, typed cells, and error phases use
the representations in `fixtures.md`. The response does not include a fixture
`format_version` wrapper because its enclosing protocol message supplies the
versioned boundary.

## 5. Isolation

Every request describes a fresh logical database. Before evaluating its
source, the adapter MUST make exactly the request catalog and snapshot visible
to the candidate implementation. It MUST NOT expose state from an earlier
request, retain mutations, add relations, or depend on request order.

The adapter may reuse process-local code, caches, compiled kernels, or device
resources only when reuse cannot change observable behavior. Fixture
relations are immutable for the request.

## 6. Failure boundary

The runner supplies only structurally and contextually valid fixture data. An
adapter must not convert any of these conditions into a ShapeSQL outcome:

- unsupported protocol or fixture version;
- malformed request or source transport;
- inability to construct the fixture database;
- adapter or candidate crash;
- timeout or cancellation;
- I/O or device failure; or
- resource exhaustion imposed by the host.

Such a condition is a protocol or infrastructure failure. The adapter should
write diagnostic detail to standard error and terminate nonzero when it cannot
continue reliably.

By contrast, a lexical, syntactic, binding, typing, or specified evaluation
failure produced while processing a valid request is returned as an ordinary
error outcome with the corresponding phase.

## 7. Version compatibility

Protocol version comparison is exact. An adapter for `0.1` MUST NOT silently
interpret a request claiming another version. Fixture versions embedded in
the request are also exact.

Version `0.1` has no negotiation or handshake message. A runner is responsible
for selecting an adapter that claims the versions declared by the suite
manifest. Later protocols may add negotiation without changing the meaning of
version `0.1` messages.
