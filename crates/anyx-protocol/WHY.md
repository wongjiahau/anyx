# Why the anyx protocol

When agents write the code, the interface is what people still review. It
says what a service promises, and every caller and implementation is checked
against it.

Every other interface format makes you translate. A JSON value names its
fields and leaves their meaning to the docs: you read the key, recall what
it means, combine it with the keys around it, and only then know what the
value says. Good names and good docs shorten that trip, but every reader
takes it on every read, people and agents alike.

anyx makes you read. A value says what it means where you're already
looking, in words a schema checks exactly.

## Records read as sentences

A record's field is a shape: words with slots for values, as in
`restart after 3 failures in a row`. A value writes each field the way it
is declared (see [Records](PROTOCOL.md#records)). Three things follow that
JSON can't do:

- **The meaning stays with the value.** No units in key names, and no
  wrapper objects that exist only to label a number.
- **One choice is one variant.** Settings that only mean something together,
  or keys that exclude each other, become a single variant such as
  `` `most commented first ``, and a combination that makes no sense can't be
  written.
- **The usual case is left out.** A checkbox is written only when on, and a
  field holding its `` `null `` default isn't written at all.

Each example below is a real API's value in JSON, the same value in anyx,
and the record that declares it.

### Kubernetes: a liveness probe

```json
{
  "httpGet": { "path": "/healthz", "port": 8080 },
  "initialDelaySeconds": 10,
  "periodSeconds": 5,
  "timeoutSeconds": 1,
  "failureThreshold": 3
}
```

```anyx value Liveness Probe
{
    `check with http get "/healthz" on port 8080,
    start checking after (10 seconds),
    check every (5 seconds),
    fail a check with no answer within (1 seconds),
    restart after 3 failures in a row,
}
```

```anyx schema
TYPE ALIAS Port: Integer . WHERE [1 <= _],
TYPE ALIAS Count: Integer . WHERE [1 <= _],

TYPE ALIAS Check: UNION [
    `check with http get (path: Text) on port (port: Port),
    `check by connecting to port (port: Port),
    # Healthy when the command exits with 0.
    `check by running (command: LIST Text),
],

RECORD Liveness Probe {
    `(check: Check),
    start checking after (Duration) DEFAULT start checking after (0 seconds),
    check every (Duration) DEFAULT check every (10 seconds),
    fail a check with no answer within (Duration)
        DEFAULT fail a check with no answer within (1 seconds),
    restart after (Count) failures in a row DEFAULT restart after 3 failures in a row,
},
```

- `initialDelaySeconds` and `periodSeconds` carry their unit in the key,
  because a JSON number has none. A `Duration` carries its own, so
  `check every (2 minutes)` can't be misread as seconds.
- `httpGet`, `tcpSocket` and `exec` are keys that exclude each other, and
  Kubernetes rejects two of them only when it validates. Here the check is
  one variant, so two can't be written.
- `failureThreshold: 3` leaves out that the failures must be in a row, and
  what happens then. `restart after 3 failures in a row` says both.
- `timeoutSeconds` hides that a check with no answer counts as a failure, so
  a slow endpoint restarts the container just as a crashed one does. The
  field now says so.

### AWS S3: a lifecycle rule

```json
{
  "ID": "logs",
  "Status": "Enabled",
  "Filter": { "Prefix": "logs/" },
  "Transitions": [
    { "Days": 30, "StorageClass": "STANDARD_IA" },
    { "Days": 90, "StorageClass": "GLACIER" }
  ],
  "Expiration": { "Days": 365 },
  "AbortIncompleteMultipartUpload": { "DaysAfterInitiation": 7 }
}
```

```anyx value Lifecycle Rule
{
    named "logs",
    apply to objects under "logs/",
    move objects [`to infrequent access at age (30 days), `to glacier at age (90 days)],
    delete objects at age (365 days),
    drop unfinished uploads after (7 days),
}
```

```anyx schema
# An object's age counts from when it was created, not from when it was
# last read.
TYPE ALIAS Storage Move: UNION [
    `to infrequent access at age (age: Duration),
    `to glacier at age (age: Duration),
    `to deep archive at age (age: Duration),
],

RECORD Lifecycle Rule {
    named (Text),
    apply to objects under (Text) DEFAULT apply to objects under "",
    move objects (LIST Storage Move) DEFAULT move objects [],
    delete objects at age (Duration?) DEFAULT delete objects at age (`null),
    drop unfinished uploads after (Duration?)
        DEFAULT drop unfinished uploads after (`null),
    # Keep the rule, but don't apply it.
    paused,
},
```

- `{ "Days": 365 }` and `{ "Prefix": "logs/" }` are objects that exist only
  to label a value. A field's words label it, so the nesting goes.
- `AbortIncompleteMultipartUpload.DaysAfterInitiation` names how S3 does it.
  `drop unfinished uploads after (7 days)` names what happens.
- `"Status": "Enabled"` is written on almost every rule. The checkbox
  `paused` is written only on the rule that is.
- `"Days": 30` doesn't say 30 days since what. `at age` does, and S3 counts
  age from creation, so logs that are read every day still move.

### Google Calendar: list events

```json
{
  "timeMin": "2026-11-01T00:00:00Z",
  "timeMax": "2026-12-01T00:00:00Z",
  "singleEvents": true,
  "orderBy": "startTime",
  "showDeleted": false
}
```

```anyx value Event Query
{
    overlapping "2026-11-01T00:00:00.000Z" to "2026-12-01T00:00:00.000Z",
    `list each occurrence by start time,
}
```

```anyx schema
TYPE ALIAS Listing: UNION [
    # A repeating event once, as its series.
    `list each series,
    `list each occurrence by start time,
    `list each occurrence by last update,
],

RECORD Event Query {
    overlapping (Instant) to (Instant),
    `(listing: Listing) DEFAULT `list each series,
    include deleted events,
},
```

- `orderBy: "startTime"` is only valid with `singleEvents: true`, which the
  API checks at run time. Here the valid pairs are the only variants.
- `singleEvents` is a famously opaque name for "expand repeating events into
  their occurrences". The variant says it.
- `timeMin` bounds an event's end and `timeMax` its start, so the range
  finds events that overlap it, not events inside it. One field with two
  slots says that, and can't be given half a range.
- `showDeleted: false` is the default, so the checkbox is left out.

### GitHub: list a repository's issues

```json
{
  "state": "open",
  "sort": "comments",
  "direction": "desc",
  "since": "2026-11-01T00:00:00Z",
  "per_page": 50
}
```

```anyx value Issue Query
{ `open, `most commented first, updated since "2026-11-01T00:00:00.000Z", 50 per page }
```

```anyx schema
TYPE ALIAS Count: Integer . WHERE [1 <= _],

TYPE ALIAS State: UNION [`open, `closed, `open or closed],

TYPE ALIAS Order: UNION [
    `newest first,
    `oldest first,
    `recently updated first,
    `least recently updated first,
    `most commented first,
    `least commented first,
],

RECORD Issue Query {
    `(state: State) DEFAULT `open,
    `(order: Order) DEFAULT `newest first,
    updated since (Instant?) DEFAULT updated since (`null),
    (Count) per page DEFAULT 30 per page,
},
```

- `sort` and `direction` only mean something together: `"sort":
  "comments", "direction": "asc"` asks the reader to work out "fewest
  comments first". Each of the six pairs is now a phrase.
- `since` filters on when an issue was last updated, not when it was
  opened. `updated since` says which.
