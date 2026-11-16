# Spec 0002: Java Syntax Subset, Lexer and Parser

Status: Accepted

## Motivation

The current `syntax` crate lexes only `class`, `{`, `}`, identifiers and whitespace, and its
parser is a brace-matching placeholder. The LSP server (see `crates/java-analyzer`) cannot
offer useful features (syntax diagnostics, outline, navigation) until the parser handles real
Java source and recovers from the errors that are always present while a user is typing.

This spec defines the token set, the CST shape, the grammar subset delivered per milestone,
the error-recovery rules, and the test strategy. It covers **syntax only**: no name
resolution, no type checking.

## Goals

1. **Lossless**: concatenating all token texts of the CST reproduces the input byte for byte,
   for any input (including invalid UTF-8-free garbage, unterminated strings, etc.).
2. **Error resilient**: parsing never panics and always terminates; malformed code yields a
   tree plus a list of syntax errors, with as much valid structure preserved as possible.
3. **Incremental-friendly**: the parse is a pure function `&str -> Parse`, suitable as a salsa
   query. Reparsing a whole file must be fast enough (target: < 10 ms for a 5k-line file in
   release mode); incremental reparse is out of scope.
4. **Delivered in vertical slices** (milestones below), each enabling a concrete IDE feature.

## Non-goals (for this spec)

- Semantic analysis, type inference, overload resolution.
- Preview or experimental language features.
- Incremental reparsing of subtrees.
- Annotation processing or build-system integration.

## Language baseline

Target **Java 21** syntax (JLS 21) as the reference grammar, delivered progressively.
The parser is **version-agnostic**: it accepts any syntactically valid Java, including
constructs from future language versions, and never reports an error merely because a
construct is newer than some language level. Grammar is added by milestone; syntax not yet
supported by the current milestone must still lex and must produce an error node rather than
corrupt surrounding structure.

## Lexer

### Contract

```rust
pub struct Token { pub kind: SyntaxKind, pub len: usize }
pub fn lex(input: &str) -> Vec<Token>;
```

- Every byte of the input belongs to exactly one token; `sum(len) == input.len()`.
- Token lengths are in bytes and always fall on `char` boundaries.
- Lexing never fails. Invalid input produces `ERROR` tokens (see below) and, where useful, a
  lexical error message recorded alongside (see "Syntax errors").
- Lexing is context-free. Contextual keywords are lexed as `IDENT` and recognised by the
  parser.

### Trivia

`WHITESPACE`, `LINE_COMMENT` (`// ...` up to but excluding the line terminator) and
`BLOCK_COMMENT` (`/* ... */`, including `/** ... */` doc comments) are trivia. The parser
attaches trivia as tokens inside the node that is open when they are encountered, with
leading trivia of a node pushed outside it where possible (see "Trivia attachment").

An unterminated block comment consumes to end of input, is still a `BLOCK_COMMENT`, and
records a lexical error.

### Tokens

Reserved keywords (each has its own `SyntaxKind` named `<UPPER>_KW`):

```
abstract assert boolean break byte case catch char class const continue default do double
else enum extends final finally float for goto if implements import instanceof int interface
long native new package private protected public return short static strictfp super switch
synchronized this throw throws transient try void volatile while _
```

Literal keywords: `true`, `false`, `null` (`TRUE_KW`, `FALSE_KW`, `NULL_KW`).

Contextual keywords (lexed as `IDENT`, interpreted by the parser only in the relevant
position): `var`, `yield`, `record`, `sealed`, `non-sealed`, `permits`, `when`, `module`,
`open`, `requires`, `exports`, `opens`, `uses`, `provides`, `to`, `with`, `transitive`.

`non-sealed` is the one hyphenated contextual keyword. The parser recognises it as the token
sequence `IDENT(non) MINUS IDENT(sealed)` with no trivia between them and wraps it in a
single `NON_SEALED_KW` composite token (see "Composite tokens").

Identifiers: `IDENT`. Start: Unicode `XID_Start`, `_` or `$`; continue: `XID_Continue` or `$`.
(`_` alone is `UNDERSCORE_KW`.)

Literals:

| Kind | Examples |
|---|---|
| `INT_NUMBER` | `0`, `42`, `1_000`, `0x1F`, `0b101`, `017` |
| `LONG_NUMBER` | `42L`, `0xFFL` |
| `FLOAT_NUMBER` | `1.5f`, `1e3F`, `.5f` |
| `DOUBLE_NUMBER` | `1.5`, `1e10`, `2d`, `0x1.8p1` |
| `CHAR_LITERAL` | `'a'`, `'\n'`, `'\u0041'` |
| `STRING_LITERAL` | `"hi"`, `"a\"b"` |
| `TEXT_BLOCK` | `"""\n  text\n  """` |

Unterminated char/string literals extend to (but exclude) the line terminator, remain
`CHAR_LITERAL` / `STRING_LITERAL`, and record a lexical error. An unterminated text block
extends to end of input and records a lexical error.

Unicode escapes (`\uXXXX`) outside literals are **not** pre-translated; they lex as
`ERROR` (see "Decisions").

Separators: `( ) { } [ ] ; , . ... @ ::`
(`L_PAREN R_PAREN L_CURLY R_CURLY L_BRACK R_BRACK SEMICOLON COMMA DOT DOT3 AT COLON2`)

Operators: `= > < ! ~ ? : -> == >= <= != && || ++ -- + - * / & | ^ % << += -= *= /= &= |= ^=
%= <<= >>= >>>=`

`>>` and `>>>` are **not** lexed as single tokens: they are lexed as consecutive `R_ANGLE`
(`>`) tokens so that nested generics (`List<List<String>>`) parse without splitting. The
parser joins adjacent `>` tokens (no trivia between them) into shift operators in expression
context via composite tokens (`SHR`, `USHR`, `SHR_EQ`, `USHR_EQ`, `GTEQ` in the same manner).

`ERROR`: any character that starts no valid token. One `ERROR` token per maximal run of
such characters is acceptable; one per char is also acceptable (tests must not depend on
which).

### Composite tokens

Some tokens are produced by the parser by merging adjacent lexer tokens (`>>`, `>>>`, `>=`
after `>`, `non-sealed`). The parser emits a single green token with the merged text and
kind. This keeps the lexer context free and the tree lossless.

## Syntax kinds

Naming follows `rust-analyzer`: `UPPER_SNAKE_CASE`, tokens first, nodes after.

- Token kinds: as listed above, plus `WHITESPACE`, `LINE_COMMENT`, `BLOCK_COMMENT`, `ERROR`.
- Node kinds: `SOURCE_FILE`, then per-milestone nodes listed below. `ERROR` is also used as
  a **node** kind wrapping tokens the parser could not place.
- Add an `#[doc(hidden)] __LAST` sentinel and replace the `transmute` in `kind_from_raw` with
  a checked conversion (assert `raw < __LAST`).

## Parser architecture

- Hand-written recursive descent with Pratt-style expression parsing (per spec 0001).
- **Event-based**: the parser produces a flat `Vec<Event>` (`StartNode{kind, forward_parent}`,
  `FinishNode`, `Token{kind, n_raw_tokens}`, `Error{msg}`), and a separate pass drives a
  `rowan::GreenNodeBuilder` from the events and the raw token stream. This allows
  `precede`-style markers (needed for binary expressions and postfix/call chains) and keeps
  trivia handling in one place.
- API:

```rust
pub struct Parse { pub green: GreenNode, pub errors: Vec<SyntaxError> }
pub struct SyntaxError { pub message: String, pub range: TextRange }
pub fn parse(text: &str) -> Parse;   // replaces today's fn returning GreenNode
```

- The parser works over a *filtered* token stream that skips trivia; the tree builder
  re-inserts trivia.
- Primitives: `current()`, `nth(n)`, `at(kind)`, `at_contextual_kw(&str)`, `bump()`,
  `eat(kind)`, `expect(kind)` (emits an error but does not consume on failure),
  `error(msg)`, `err_and_bump(msg)`, `err_recover(msg, recovery_set)`.
- **Progress guarantee**: every loop iteration must consume at least one token or exit. A
  debug assertion (step counter) fails the test run on any parser that spins.
- **No panics**: `parse` must not panic for any input. Enforced by fuzz/property tests.

### Trivia attachment

- Trivia between tokens belongs to the innermost node that is open *at the time the next
  non-trivia token is consumed*, except:
  - leading trivia before the first token of a node is attached to the **parent** (i.e. the
    node starts at its first non-trivia token);
  - trailing trivia at the end of a node is attached to the **parent** (i.e. the node ends at
    its last non-trivia token).
- Comments directly preceding a declaration (doc comments) are therefore siblings of the
  declaration node. A later AST layer finds them by walking previous siblings.

## Error recovery

General rules:

1. **Never drop tokens**: unplaceable tokens are wrapped in an `ERROR` node (or attached
   as-is) so the tree stays lossless.
2. `expect(kind)` that fails records a *missing* error as a zero-width range at the end of the
   previous non-trivia token (where the missing token would be typed), and does **not**
   consume anything. *Unexpected* tokens are reported on the range of the token that is
   about to be skipped.
3. Each list/block parser has a **recovery set**: tokens at which it stops consuming
   unexpected input and returns to the caller. Initial sets:
   - Member level (inside class body): `}`, modifiers, `class`, `interface`, `enum`,
     `record`, `@`, type-starting tokens, `void`, `{`, `;`.
   - Statement level (inside a block): `}`, `;`, statement-starting keywords, `{`.
   - Top level: `package`, `import`, modifiers, type-declaration keywords, `;`.
4. A stray `}` at top level or member level is consumed into an `ERROR` node. A missing `}`
   is reported at end of file at the block's opening brace location plus the EOF point; the
   enclosing structure still closes.
5. Unclosed `(`/`[`/`<` stop at the recovery set of the enclosing construct (so a half-typed
   method call does not swallow the rest of the class).
6. Half-typed constructs must still produce the named node (e.g. `foo.` followed by `}`
   yields a `FIELD_ACCESS_EXPR` with a missing name plus an error). This is what makes
   completion work later.

## Milestones

Each milestone ships with: node kinds, grammar, tests (inline CST snapshots), and the IDE
feature it unlocks.

### M1: Lexer completion + compilation unit skeleton

*Unlocks*: syntax diagnostics on broken files without cascading errors.

Lexer: everything in the "Lexer" section above.

Parser nodes: `SOURCE_FILE`, `PACKAGE_DECL`, `IMPORT_DECL`, `NAME`, `NAME_REF`, `PATH`,
`MODIFIER_LIST`, `ANNOTATION` (name + optional unparsed balanced `( ... )` as
`ANNOTATION_ARGS`), `CLASS_DECL`, `INTERFACE_DECL`, `ENUM_DECL`, `RECORD_DECL` (components as
an unparsed balanced `RECORD_COMPONENT_LIST`), `ANNOTATION_TYPE_DECL`, `CLASS_BODY` (with
`{ }` and recovery), `EXTENDS_CLAUSE`, `IMPLEMENTS_CLAUSE`, `PERMITS_CLAUSE`,
`MEMBER_PLACEHOLDER`, `ERROR`, and the minimal type syntax that declaration headers need,
pulled forward from M2: `TYPE_PARAMS`, `TYPE_PARAM`, `TYPE_ARGS`, `PATH_TYPE`,
`PRIMITIVE_TYPE`, `ARRAY_TYPE`, `WILDCARD_TYPE`.

`EOF` is a parser-only pseudo token kind and never appears in a tree. Modifier lists live
inside the declaration node they belong to, and `sealed` / `permits` / `record` stay `IDENT`
tokens in the tree (only `non-sealed` becomes a composite `NON_SEALED_KW`).

Grammar (informal; `?` optional, `*` repetition):

```
SourceFile      = PackageDecl? ImportDecl* TypeDecl*
PackageDecl     = Annotation* 'package' Path ';'
ImportDecl      = 'import' 'static'? Path ('.' '*')? ';'
TypeDecl        = Modifiers? (ClassDecl | InterfaceDecl | EnumDecl | RecordDecl | AnnotationTypeDecl) | ';'
ClassDecl       = 'class' Name TypeParams? ('extends' Type)? ('implements' TypeList)? ('permits' TypeList)? ClassBody
InterfaceDecl   = 'interface' Name TypeParams? ('extends' TypeList)? ('permits' TypeList)? ClassBody
Modifiers       = (Annotation | modifier-keyword | 'sealed' | 'non-sealed')*
```

Member bodies in M1 are parsed *shallowly*: each member is consumed as a run of tokens up to
and including a `;` outside braces, or a braced block that is not an initializer (a `{ ... }`
after `=` keeps going to the `;`), wrapped in a `MEMBER_PLACEHOLDER` node with its modifiers.
Nested type declarations are real nodes. Enum constants are placeholders too, and the
missing-`;` error is suppressed in enum bodies. This keeps the declaration structure correct
and recovery sound while M2 is built.

Acceptance:
- All fixtures in `crates/syntax/test_data/ok/m1/*.java` parse with zero errors.
- All fixtures in `.../err/m1/*.java` parse with the errors listed in the adjacent
  `.expected` snapshot and never panic.
- Roundtrip property holds for every fixture and for 10k random byte/char mutations of them.

### M2: Members, types and basic statements

Statements contain expressions, which arrive in M3, so M2 is delivered in two steps.

#### M2a: Members and parameters (declaration level)

*Unlocks*: document symbols (outline).

Nodes: `FIELD_DECL` (with one `VARIABLE_DECLARATOR` per declared name), `METHOD_DECL`,
`CONSTRUCTOR_DECL` (including compact record constructors), `INITIALIZER`, `PARAM_LIST`,
`PARAM`, `THROWS_CLAUSE`, `DEFAULT_VALUE` (annotation method defaults), `ENUM_BODY`,
`ENUM_CONSTANT`, `RECORD_COMPONENT` (children of the existing `RECORD_COMPONENT_LIST`).
`void` is a `PRIMITIVE_TYPE`.

Grammar (informal):

```
Member          = ';' | Initializer | Modifiers? (TypeDecl | Constructor | Method | Field)
Initializer     = 'static'? Block
Constructor     = TypeParams? Name ParamList Throws? Body | Name Block     (compact form)
Method          = TypeParams? Type Name ParamList Dims? Throws? ('default' Value)? Body
Field           = Type Declarator (',' Declarator)* ';'
Declarator      = Name ('[' ']')* ('=' Initializer)?
Body            = Block | ';'
Param           = Modifiers? Type '...'? Name ('[' ']')*
EnumBody        = '{' (EnumConstant (',' EnumConstant)* ','?)? (';' Member*)? '}'
EnumConstant    = Annotation* Name ArgList? ClassBody?
```

M2a was delivered with two stand-ins, both since replaced: method, constructor and initializer
bodies were brace-balanced token runs (now real statements, M2b), and field initializers,
annotation defaults and enum constant arguments were `EXPR_PLACEHOLDER` token runs (now real
expressions, M3). A body still open when `public`, `private` or `protected` shows up where a
statement should start is treated as unclosed (these can never start a statement), so one
missing `}` does not swallow the rest of the class; that rule remains.

Member-level recovery: a member that cannot start with a type, a name, a modifier or a block is
wrapped in an `ERROR` node up to the next `;` (consumed), `}`, modifier, annotation or type
declaration keyword. Missing pieces are reported without consuming anything (`expected ';'`,
`expected a name`, `expected ')'`, `expected '{' or ';'`).

Acceptance:

- every class, interface, enum, record, field, method, constructor and enum constant in the
  fixture set appears in the CST with its name in a `NAME` node;
- `ide::document_symbols` lists them hierarchically with correct ranges (each field declarator
  is its own symbol; record components are fields of the record; declarations without a name
  are left out and their members hoisted), and the LSP server serves
  `textDocument/documentSymbol`;
- error fixtures keep unaffected members intact.

#### M2b: Statements

*Unlocks*: recovery inside method bodies (a broken statement no longer swallows the class) and a
real block structure for M3.

Nodes: `BLOCK`, `LOCAL_VAR_DECL`, `EXPR_STMT`, `EMPTY_STMT`, `IF_STMT`, `WHILE_STMT`, `DO_STMT`,
`FOR_STMT`, `FOREACH_STMT`, `RETURN_STMT`, `BREAK_STMT`, `CONTINUE_STMT`, `THROW_STMT`,
`YIELD_STMT`, `ASSERT_STMT`, `SYNCHRONIZED_STMT`, `LABELED_STMT`, `TRY_STMT`, `RESOURCE_LIST`,
`RESOURCE`, `CATCH_CLAUSE`, `FINALLY_CLAUSE`, `SWITCH_STMT`, `SWITCH_BODY`, `SWITCH_CASE`,
`SWITCH_LABEL`. Local class, interface, enum and record declarations reuse the member-level
declaration nodes. M2b embedded `EXPR_PLACEHOLDER` token runs wherever a statement contains an
expression; M3 replaced them with real expressions.

Grammar notes:

- **Declaration or expression?** A statement starting with a type that is directly followed by
  an identifier (`Foo x`, `List<String> xs`, `int[] a`) is a declaration; nothing else can
  look like that. The type is recognised by a bounded lookahead over tokens that may occur in a
  type, so `a < b` is never mistaken for generic arguments. A primitive type keyword not
  followed by `.`, `[` or `::` is always a declaration (`int ;` is a declaration with a
  missing name).
- `yield` is a contextual keyword: it starts a yield statement unless it is followed by an
  assignment operator, `.`, `[`, `++`, `--`, `:`, `;`, `}` or end of input.
- `switch` bodies hold `SWITCH_CASE` nodes, each a `SWITCH_LABEL` (`case ...:` / `default:` /
  `case ... ->`) followed by its statements, or for the arrow form by exactly one statement. A
  statement before the first label is reported as `expected 'case' or 'default'`.
- `try` requires a `catch`, a `finally` or a resource list.

Statement-level recovery:

- A block that is still open at `public`, `private` or `protected` (they can never start a
  statement) is unclosed; every enclosing open block reports its own `expected '}'`.
- `else`, `catch`, `finally`, `case` and `default` where a statement should start are reported
  as `unexpected '<keyword>'` and consumed into an `ERROR` node.
- A control-flow construct whose body is missing reports `expected a statement` and consumes
  nothing.
- Missing `(`, `)`, `;`, `{`, `while`, `:` and `->` are reported at the end of the previous
  token without consuming input.

### M3: Expressions

*Unlocks*: go-to-definition for locals and members, completion after `.`. Replaces every
`EXPR_PLACEHOLDER` (the node kind is removed).

Nodes: `LITERAL`, `NAME_REF_EXPR` (wrapping a `NAME_REF`), `FIELD_ACCESS_EXPR`,
`METHOD_CALL_EXPR` (`receiver . TYPE_ARGS? NAME_REF ARG_LIST`), `CALL_EXPR` (`callee ARG_LIST`
for unqualified `foo(..)`, `this(..)` and `super(..)`), `ARG_LIST`, `NEW_EXPR`, `ARRAY_INIT`,
`ARRAY_ACCESS_EXPR`, `UNARY_EXPR` (prefix), `POSTFIX_EXPR` (`++`, `--`), `BINARY_EXPR`,
`ASSIGN_EXPR`, `TERNARY_EXPR`, `CAST_EXPR`, `INSTANCEOF_EXPR`, `LAMBDA_EXPR`, `LAMBDA_PARAMS`,
`METHOD_REF_EXPR`, `PAREN_EXPR`, `THIS_EXPR`, `SUPER_EXPR` (either may be qualified:
`Outer.this`), `SWITCH_EXPR`, `CLASS_LITERAL`, and the pattern nodes `TYPE_PATTERN`,
`RECORD_PATTERN` and `GUARD`. Patterns moved here from M4 because `instanceof` and `case`
labels need them.

Precedence (low to high): assignment (right-assoc), ternary (right-assoc), `||`, `&&`, `|`,
`^`, `&`, equality, relational/`instanceof`, shift, additive, multiplicative, unary/cast,
postfix. Binary operators are left-associative. Lambdas are operands that extend as far as
possible.

Ambiguities and how they are resolved (each has dedicated fixtures):

- **Shifts and `>=`.** The lexer never combines `>`. In expression position the parser merges
  adjacent tokens with no trivia between them into `GTEQ`, `SHR`, `USHR`, `SHREQ` and `USHREQ`
  composite tokens; `a > > b` stays two operators.
- **Generic arguments vs. less-than.** In expression position `<` is always less-than. Types
  appear only after `new`, `instanceof`, in casts, after `.` (explicit method type arguments:
  `Foo.<T>bar()`), and as `Type::name` / `Type.class` when the type has type arguments or
  array dimensions (`List<String>::new`, `int[]::new`, `Foo[].class`), which a bounded token
  lookahead recognises.
- **Cast vs. parenthesised expression.** `(Type) operand` is a cast when the parenthesised
  text is a type (or an intersection `A & B`) and either it is a single primitive keyword, or
  the next token can start an operand that is not `+`, `-`, `++` or `--` (`(a) + b` is a sum, as
  in Java).
- **Lambda vs. parenthesised expression.** `( ... ) ->` and `ident ->` are lambdas; the
  parentheses are matched with a bounded lookahead. Inside `case` labels lambdas are not
  recognised, so `case FOO ->` is a rule and not a lambda.
- **Switch labels.** An item of `case a, b ->` is a pattern if it starts with a modifier or
  annotation, or is a type followed by an identifier or `(`; otherwise it is an expression.
  `default` may appear as an item (`case null, default`). A `when` guard follows the items.
- **Contextual `var`/`yield`/`record`** stay identifiers except where M2b/M1 say otherwise.

Recovery: a missing operand is reported as `expected an expression` without consuming input.
`foo.` followed by anything that is not a name still yields a `FIELD_ACCESS_EXPR` (with the
error `expected a name`), so completion has a node to work from. Argument lists, array
initializers and parameter lists stop at `;`, `}` and the member-only modifiers, so a
half-typed call does not swallow the next declaration.

Robustness limits (the parser must neither overflow the stack nor build trees that overflow it
when dropped): nesting of expressions, statements, class bodies and type arguments is limited to
256 levels (`nesting is too deep` is reported once and the offending tokens become `ERROR`
nodes or errors), and a single left-associative chain (`a + b + ...`, `a.b.c...`, `f()()...`)
is limited to 1024 links (`expression is too long`). Token lookahead scans stop after 512
tokens.

Still unparsed: annotation arguments (`ANNOTATION_ARGS` is a balanced token run), and type
annotations.

### M4: Modern Java

Annotation arguments and element values, text block escape validation, module declarations
(`module-info.java`), type annotations, generic constructor type arguments (`new <T>Foo()`),
and any construct the JDK sources still show to be unsupported.

## Syntax errors surfaced to the IDE

- `Parse.errors` is the single source of syntax diagnostics (lexical errors are merged in,
  ordered by start offset).
- Messages are short, lowercase, and actionable: `expected ';'`, `expected a type`,
  `unclosed block`, `unterminated string literal`.
- `hir` exposes a salsa query `syntax_errors(file_id) -> Arc<Vec<SyntaxError>>`; `ide`
  converts these to `Diagnostic`; the LSP crate converts to `publishDiagnostics` with UTF-16
  ranges (reuse `convert.rs`).

## Test strategy

1. **Text fixtures (the main corpus)**, inspired by the sibling `bourbon-lang` scanner tests.
   Two layouts exist, one per phase. Both are regenerated with `UPDATE_EXPECT=1` and the diff
   is reviewed like a snapshot, and both collect every mismatch and report each as a unified
   diff, so one failing fixture does not hide the others.

   **Parser fixtures** are *pairs of files* in `crates/syntax/test_data/parser/{ok,err}/`:

   - `name.java` is the source, verbatim (any file name stem; numeric prefixes keep order).
   - `name.scm` is the expected tree as an S-expression (Scheme syntax, hence the extension).
     `name.trivia.scm` instead means whitespace and comments are part of the expected tree;
     without it they are omitted from the expectation (the lossless invariant still covers
     them).
   - Nodes are `(KIND child...)` and tokens are `(KIND "text")`, with text in Rust `{:?}`
     escaping. `;` starts a comment, which is allowed in hand-written files but discarded when
     regenerating.
   - A trailing `(errors (error "message" "line:col..line:col") ...)` form lists every lexical
     and syntax error with its range (1-based; columns count `char`s). No such form means no
     errors.
   - `ok/` fixtures must parse without errors and `err/` fixtures must produce at least one.
   - Expected files are compared in canonical printed form, so hand-written files may be laid
     out freely. Lists with at most one nested list child stay on one line when regenerated.
   - A form `(!! "message")` is how the harness reports a violated invariant. It never
     matches and is never written by `UPDATE_EXPECT=1`.
   - A `.java` file without an expected file, or an expected file without a `.java` file, is a
     failure.

   **Lexer fixtures** are text files in `crates/syntax/test_data/lexer/*.txt`, because lexer
   cases are tiny and many fit one file. A file holds one or more cases:

   ```
   =====
   Case title
   =====
   <source code under test, verbatim>
   ---
   <expected output, one item per line>
   ```

   - A case starts at a line of three or more `=`, a title line, and another `=` line. The
     source runs to the first line of three or more `-`. The expected output runs to the next
     case header or end of file.
   - The source is the lines between the two rules, each terminated by `\n` (an empty source
     is the empty string).
   - Text after the dashes of the separator holds options, e.g. `--- trivia` includes trivia
     tokens in the lexer output (omitted by default).
   - **Lexer output** is one line per token, `KIND "lexeme" @ line:col` (1-based line,
     1-based column counted in `char`s, lexeme in Rust `{:?}` escaping), followed directly by
     `error: <message> @ line:col` for any lexical error starting at that token.
   - Unlike bourbon, expectations carry no line-synchronised `---` markers and are
     **generated, not hand-aligned**. Expected sections must not contain comments (they are
     discarded on update).
2. **Inline unit tests** for properties that are awkward in fixtures (e.g. `>` never combining
   with a following `>`/`=`, losslessness on arbitrary input), plus small `expect-test`
   snapshots (`check(src, expect![[...]])` style) where a fixture would be overkill.
3. **Lexer/parser unit tests** may still assert on specific token kinds directly when that is
   clearer than a fixture.
4. **Invariants checked on every fixture and in property tests**:
   - Roundtrip: `SyntaxNode::new_root(green).text() == input`.
   - No panic and termination (with a step budget).
   - Every `ERROR` node is non-empty, or accompanied by a recorded error.
5. **Fuzz-ish property tests** (a seeded loop, no extra dependencies, no nightly): every prefix,
   every suffix and every single-character deletion of every parser fixture source, plus
   thousands of generated fragment soups, must satisfy the invariants and error-range sanity
   (ordered, in bounds, on `char` boundaries).
6. **Real-world smoke test** (opt-in, not in default CI): parse a local JDK `src.zip` or any
   configured Java source tree and report files with errors; used to track grammar coverage
   from M2 on. Valid JDK sources should parse with zero syntax errors by the end of M3.

## Impacted code

- `crates/syntax`: `tests/` (fixture harness), `test_data/` (fixtures), `syntax_kind.rs` (many
  new kinds, `__LAST`), `lexer.rs`, new `parser/` module (`grammar/`, `event.rs`, `tree_builder.rs`, `token_set.rs`), `lib.rs`
  (`Parse`, `SyntaxError`).
- `crates/hir`: `parse` query returns `Parse`/green node, new `syntax_errors` query.
- `crates/ide`: `diagnostics(db, file_id)`.
- `crates/java-analyzer`: publish diagnostics on open/change/close.

## Decisions

1. **Unicode escapes** outside literals (`\u0041` in identifiers etc.): deferred. They lex as
   `ERROR` for now; revisit if real-world sources need them.
2. **Language level**: no gating. Accept any valid Java syntax, including future syntax.
3. **Doc comments**: siblings of the declaration node, matching `rust-analyzer`.
4. **Lexer**: hand-written, consistent with spec 0001.
5. **Snapshot tooling**: text fixtures with `UPDATE_EXPECT=1` regeneration for the corpus (see
   "Test strategy"), and the `expect-test` crate for small inline snapshots.

## Immediate next steps

1. Write M1 lexer tests (failing), implement lexer changes.
2. Introduce `Parse`/`SyntaxError`, the event-based parser skeleton and tree builder, with
   the `kind_from_raw` safety fix.
3. Write M1 snapshot and fixture tests, implement M1 grammar.
4. Wire `syntax_errors` through `hir`/`ide` to `publishDiagnostics` in the LSP crate.
