# CA2 Reflection

### 1. The dangling `else`

In my `if_statement` function, the resolution of the dangling `else` happens at **`src/parser.rs:150`**, where the `then_branch` is parsed recursively by calling `self.statement()`. 

The grammar does not explicitly state which `if` an `else` belongs to; it only says `ifStmt → "if" "(" expression ")" statement ( "else" statement )?`. However, recursive descent handles this for free. When the parser sees the first `if`, it calls `self.statement()` for the body. If that body is another `if`, it recursively enters `if_statement` again. The inner `if` finishes parsing its own statement, then immediately checks for an `else`. If it finds one, it greedily consumes it. 

For `if (false) if (true) print "a"; else print "b";`, my parser prints `(if false (if true (print a) (print b)))`. The `else` is captured by the inner `if`. If it bound to the outer `if` instead, it would print `(if false (if true (print a)) (print b))`. The recursion guarantees the `else` always matches the nearest unmatched `if`, resolving the ambiguity automatically.

### 2. Assignment is not LL(1)

In my `assignment` function, I check what the left side turned out to be at **`src/parser.rs:206`**, specifically the check `if let Expr::Variable { name, .. } = expr`. 

This check cannot happen before the left side is parsed because when the parser sees an identifier like `a`, it does not yet know if it is parsing a variable read (`a + 1`) or the start of an assignment (`a = 1`). The grammar rule `IDENTIFIER "=" assignment | logic_or` is not LL(1) because both alternatives begin with `IDENTIFIER`. To solve this, I parse the left side as an ordinary `or()` expression first, and only if the next token is `=` do I look back at what I just parsed. If it is a valid variable, I build an `Assign` node; otherwise, I emit an error. Because of this look-back, the input `1 = 2;` produces the exact message from section 5.1: `Cannot assign to this expression.`

### 3. Synchronisation

My `synchronise` implementation is at **`src/parser.rs:387`**. After a parse error, it always discards at least one token to guarantee progress, then keeps discarding until it has just consumed a `;` or is positioned on one of the statement keywords: `fun`, `var`, `if`, `while`, `print`, or `return`. I stop on these keywords because they are the only tokens that can reliably begin a new statement, meaning the parser can safely resume there. I stop after a `;` because it marks a completed statement.

If I deleted the call to `synchronise()` in the main `parse` loop, the test `tests/phase-2/invalid/expect_function_name.kobo` would hang. Instead of discarding the bad token and moving on, the parser would get stuck in an infinite loop, repeatedly failing on the exact same token and eventually timing out the test runner (as I discovered during development). 

**History anchor for Question 3:**
The commit where `synchronise` was still wrong (lacking the guarantee of the first discard) was **`fc188c0`**. 
The line stood as:
```rust
while !self.at_end() {
    if !self.at_end() {
    self.current += 1;
}
```