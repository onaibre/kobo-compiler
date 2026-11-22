# CA3 Reflection

## 1. Short-circuiting `and` and `or`

`src/interpreter.rs` lines **134–156** are the `Expr::Logical` arm of `evaluate`. The `Or` case (around `src/interpreter.rs:137`) evaluates the left operand, and if it is truthy returns it **unchanged**; otherwise it evaluates and returns the right operand. The `And` case (`src/interpreter.rs:145`) evaluates the left operand, and if it is falsey returns it unchanged; otherwise it evaluates and returns the right operand. In both cases the value that comes back is **one of the operands**, never a coerced boolean.

For `nil or "fallback"`, `nil` is falsey (spec 2.1), so the `Or` arm evaluates the right operand and returns `"fallback"`. For `0 and 1`, `0` is truthy (only `nil` and `false` are falsey), so the `And` arm evaluates the right operand and returns `1`.

Returning `true`/`false` instead would pass tests that only check truthiness in a condition (`if (a or b) …`), because both engines coerce the operand at the branch anyway. It would fail the tests that print the result directly: `print nil or "fallback";` must print `fallback`, not `true`, and `print 0 and 1;` must print `1`, not `true`. The spec is explicit that the operators yield an operand, so the tree walk and the VM must both leave one on the stack.

## 2. `lookup` and `end_scope`

`src/resolver.rs:76` is the innermost-first scan in `lookup`. It walks `self.locals` from the end and returns `Resolved::Local { depth, slot }` for the nearest initialised binding of the name, or `Resolved::Global` if none is found. `src/compiler.rs:230` is `end_scope`, which emits one `Op::Pop` per local that died in the block, using the count the resolver stored on the `Block` node.

Resolving a name to a slot at compile time makes the VM faster than the tree walk because the tree walk must, at every variable read, search the `Environment`'s `Vec<HashMap>` from the innermost scope outward until it finds the name. That is O(depth) hash lookups per read. The VM does one array index, `stack[base + slot]`, with the slot fixed in the instruction at compile time. There is no search and no hashing at run time.

If `end_scope` emits one `Pop` too few, the stack grows by one extra value on every loop iteration that declares a block-local. That extra value sits between `base` and the slots that `GetLocal`/`SetLocal` address, so every subsequent local read shifts by one. Phase 4 catches it — `locals_slots.kobo` and `jumps_while.kobo` both fail, because they exercise block locals inside loops. Phase 3 does not catch it, because the tree walk never uses slots: the resolver's depth is independent of the VM's stack height, so a missing `Pop` is invisible to the interpreter. That asymmetry is exactly what the brief warns about.

## 3. The extension: `for` loops

My extension is `for` loops (row 0 of the matric table). I added a `TokenType::For` variant and its keyword mapping in `src/token.rs` (around `src/token.rs:41` and `src/token.rs:112` in my tree), and a `for_statement` method in `src/parser.rs` (around `src/parser.rs:280`) plus a `TokenType::For =>` arm in `statement` (`src/parser.rs:198`).

The back end got the feature for free because `for_statement` **desugars** the loop into the statement nodes that already exist. It builds a `Stmt::Block` containing the initialiser and a `Stmt::While` whose body is a `Stmt::Block` of the original body followed by the increment. Both blocks carry `locals: 0`, which the resolver overwrites in `Stmt::Block`'s arm. Because the desugared tree contains only `Block`, `While`, `Expression`, and `Var`, the interpreter, the compiler, and the VM all handle it with the code they already had — I changed none of the three.

Anchor in my own history: in commit **`57d645b`** ("update build fails"), the `for` arm was still the placeholder `Stmt::Expression(Expr::Literal { value: Value::Bool(true), line: 0 })` — the parser advanced past `for` but emitted a no-op, so `for (var i = 0; i < 3; i = i + 1) print i;` compiled and printed nothing. In commit **`3318240`** ("Fixed all bugs"), the arm reads `self.advance(); self.for_statement()` and `for_statement` builds the desugared `Block`/`While` tree shown above.