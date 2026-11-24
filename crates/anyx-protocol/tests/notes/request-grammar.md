# Request grammar diagnostics (#25)

The fixed grammar reports syntax errors before schema checking. Semantic
errors still name the method parameter and record field. These are the
intentional changes to existing error expectations:

| Case | Change and reason |
| --- | --- |
| `read "a", "loose"` (`request_errors.txt`) | Names the unexpected string and expects a call or the end of the document. A string is invalid request syntax independently of the schema's methods. |
| `count { counts { hymn 3 } }` (`map_errors.txt`) | Names the typed syntax as a record field where a map entry with a string key is required. The example includes the `⁽` needed for word values. |
| `count { counts { "hymn" } }` (`map_errors.txt`) | The grammar expects an entry's key and value, rather than the checker interpreting a lone string as an invalid map entry. |
| `wait { "tea" 5 minutes }` (`map_errors.txt`) | Points at the extra `minutes` word (column 16) instead of the entry's start (column 8). Write `"tea" ⁽5 minutes` to make the duration the entry's value. |
| `set { at "2026-11-14T06:00:08Z", every 5 minutes }` (`time_errors.txt`) | Points at the extra `minutes` word (column 42) instead of the field's start (column 34). Write `every ⁽5 minutes` to make the duration the field's value. |
| `apply { mode dark }` (`value_errors.txt`) | Points at the missing value at the end of the field call. Without `⁽`, both words form its name. Write `mode ⁽dark`. |
| `tag ([] AS ⁽LIST ⁽TEXT)` (`overloading.rs`) | Points at the invalid `TEXT` keyword within the type rule, rather than treating the whole `LIST ⁽TEXT` as an unresolved type. Retains the Title Case hint. |

The nested-annotation explanation, unknown-type explanations, overload
resolution, numeric type errors, duration semantic errors, and all valid
request/response snapshots are preserved. The grammar intentionally has overlapping readings for durations and
neutral value calls. The snapshot test pins this generator warning;
the checker preserves the choice-variant interpretation of duration shapes.
