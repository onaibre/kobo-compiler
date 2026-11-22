use crate::ast::{Expr, Program, Resolved, Stmt};
use crate::environment::Environment;
use crate::token::{Token, TokenType};
use crate::value::Value;

pub struct RuntimeError {
    pub line: usize,
    pub message: String,
}

/// Why a statement stopped early: a runtime error unwinding to `main`, or a `return`
/// unwinding to its call.
enum Flow {
    Error(RuntimeError),
    Return(Value),
}

type Exec<T> = Result<T, Flow>;

/// Kobo's call stack is Rust's, and overflowing it aborts with no message, so calls are
/// capped (spec section 5.1). The measured limit here is near 1,500; 500 leaves room for
/// a deeply nested expression inside every frame.
pub const MAX_FRAMES: usize = 500;

pub fn interpret(program: &Program) -> Result<(), RuntimeError> {
    let mut env = Environment::new();
    for stmt in &program.stmts {
        match execute(stmt, &mut env, program) {
            Ok(()) => {}
            // `return` outside any function ends the program (spec section 3.1).
            Err(Flow::Return(_)) => return Ok(()),
            Err(Flow::Error(e)) => return Err(e),
        }
    }
    Ok(())
}

fn execute(stmt: &Stmt, env: &mut Environment, program: &Program) -> Exec<()> {
    match stmt {
        Stmt::Expression(e) => { evaluate(e, env, program)?; Ok(()) }
        Stmt::Print(e) => {
            let v = evaluate(e, env, program)?;
            println!("{}", v);
            Ok(())
        }
        Stmt::Var { name, initialiser, .. } => {
            let v = match initialiser {
                Some(e) => evaluate(e, env, program)?,
                None => Value::Nil,
            };
            env.declare(&name.lexeme, v);
            Ok(())
        }
        Stmt::Block { body, .. } => {
            env.push_scope();
            let result = (|| {
                for s in body {
                    execute(s, env, program)?;
                }
                Ok(())
            })();
            env.pop_scope();
            result
        }
        Stmt::If { condition, then_branch, else_branch } => {
            let c = evaluate(condition, env, program)?;
            if c.is_truthy() {
                execute(then_branch, env, program)
            } else if let Some(alt) = else_branch {
                execute(alt, env, program)
            } else {
                Ok(())
            }
        }
        Stmt::While { condition, body } => {
            while evaluate(condition, env, program)?.is_truthy() {
                execute(body, env, program)?;
            }
            Ok(())
        }
        Stmt::Function { name, .. } => {
            // Function values are indexed; store the index as a Value.
            let index = program.functions.iter().position(|f| f.name == name.lexeme)
                .expect("resolver/parser kept function order");
            env.declare(&name.lexeme, Value::Function(index));
            Ok(())
        }
        Stmt::Return(value) => {
            let v = match value {
                Some(e) => evaluate(e, env, program)?,
                None => Value::Nil,
            };
            Err(Flow::Return(v))
        }
    }
}

fn evaluate(expr: &Expr, env: &mut Environment, program: &Program) -> Exec<Value> {
    // TODO(you): spec 2 and 4.1: evaluate one expression to a Value. Truthiness, equality
    //            across kinds and short-circuiting are all specified.
    todo!("evaluate")
}

/// Run a function body in a fresh frame: parameters in the frame's own scope, the
/// globals beyond it, and the frame popped on every way out.
fn call(
    index: usize,
    args: Vec<Value>,
    line: usize,
    env: &mut Environment,
    program: &Program,
) -> Exec<Value> {
    // TODO(you): spec 4.3: check arity, push a frame, declare the parameters, run the body, and
    //            pop the frame on all three ways out.
    todo!("call")
}

fn numbers(l: Value, r: Value, line: usize) -> Exec<(f64, f64)> {
    match (l, r) {
        (Value::Number(a), Value::Number(b)) => Ok((a, b)),
        _ => Err(err(line, "Operands must be numbers.")),
    }
}

fn arith(l: Value, r: Value, line: usize, f: fn(f64, f64) -> f64) -> Exec<Value> {
    let (a, b) = numbers(l, r, line)?;
    Ok(Value::Number(f(a, b)))
}

fn compare(l: Value, r: Value, line: usize, f: fn(f64, f64) -> bool) -> Exec<Value> {
    let (a, b) = numbers(l, r, line)?;
    Ok(Value::Bool(f(a, b)))
}

fn undefined(name: &Token) -> Flow {
    err(name.line, &format!("Variable '{}' is not defined.", name.lexeme))
}

fn err(line: usize, message: &str) -> Flow {
    Flow::Error(RuntimeError {
        line,
        message: message.to_string(),
    })
}
