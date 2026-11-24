# Anyx syntax and parsing

- Never layer custom parsing over the tree produced by a parser generated with the Anyx parser generator. Express syntax in the Anyx grammar and consume the generated tree as-is for semantic work such as name resolution, type checking, and evaluation.
- Use Anyx’s existing grouping syntax when an expression needs nested calls. Use `₎` for left grouping and `⁽` for right grouping. Attach `₎` to the operand before it and `⁽` to the operand after it. For example, `new revision = ⁽revision + x₎ + 1` produces the same call tree as `new revision = ((revision + x) + 1)`. The ungrouped `new revision = revision + 1` produces the flat shape `_ = _ + _`. Do not split or regroup that shape in consumer code to invent precedence. The flat shape can be declared and interpreted as a call in its own right.

# Design priorities

- Readability over writability, in anyx and in every language built on it, such as the protocol. Code is read far more than written, and mostly written by AI agents now. Extra parentheses that keep the call tree honest, as in `(any) case`, cost only writability, so prefer them over a new keyword or a shortcut that reinterprets the tree.
