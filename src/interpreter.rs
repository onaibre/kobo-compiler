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
    match expr {
        Expr::Literal { value, .. } => Ok(value.clone()),

        Expr::Grouping(inner) => evaluate(inner, env, program),

        Expr::Variable { name, at } => match at {
            Resolved::Local { depth, .. } => {
                env.get_at(*depth, &name.lexeme)
                    .cloned()
                    .ok_or_else(|| undefined(name))
            }
            Resolved::Global => env
                .get_global(&name.lexeme)
                .cloned()
                .ok_or_else(|| undefined(name)),
        },

        Expr::Assign { name, value, at } => {
            let v = evaluate(value, env, program)?;
            let ok = match at {
                Resolved::Local { depth, .. } => env.assign_at(*depth, &name.lexeme, v.clone()),
                Resolved::Global => env.assign_global(&name.lexeme, v.clone()),
            };
            if ok { Ok(v) } else { Err(undefined(name)) }
        }

        Expr::Unary { operator, right, .. } => {
            let v = evaluate(right, env, program)?;
            match operator.token_type {
                TokenType::Minus => match v {
                    Value::Number(n) => Ok(Value::Number(-n)),
                    _ => Err(err(operator.line, "Operand must be a number.")),
                },
                TokenType::Bang => Ok(Value::Bool(!v.is_truthy())),
                _ => unreachable!(),
            }
        }

        Expr::Binary { left, operator, right } => {
            let l = evaluate(left, env, program)?;
            let r = evaluate(right, env, program)?;
            let line = operator.line;
            match operator.token_type {
                TokenType::Plus => match (l, r) {
                    (Value::Number(a), Value::Number(b)) => Ok(Value::Number(a + b)),
                    (Value::Str(a), Value::Str(b)) => Ok(Value::Str(a + &b)),
                    _ => Err(err(line, "Operands must be two numbers or two strings.")),
                },
                TokenType::Minus => arith(l, r, line, |a, b| a - b),
                TokenType::Star => arith(l, r, line, |a, b| a * b),
                TokenType::Slash => {
                    let (a, b) = numbers(l, r, line)?;
                    if b == 0.0 { return Err(err(line, "Division by zero.")); }
                    Ok(Value::Number(a / b))
                }
                TokenType::EqualEqual => Ok(Value::Bool(equal(&l, &r))),
                TokenType::BangEqual => Ok(Value::Bool(!equal(&l, &r))),
                TokenType::Greater => compare(l, r, line, |a, b| a > b),
                TokenType::GreaterEqual => compare(l, r, line, |a, b| a >= b),
                TokenType::Less => compare(l, r, line, |a, b| a < b),
                TokenType::LessEqual => compare(l, r, line, |a, b| a <= b),
                _ => unreachable!(),
            }
        }

        Expr::Logical { left, operator, right } => {
            let l = evaluate(left, env, program)?;
            match operator.token_type {
                TokenType::Or => {
                    if l.is_truthy() { Ok(l) } else { evaluate(right, env, program) }
                }
                TokenType::And => {
                    if !l.is_truthy() { Ok(l) } else { evaluate(right, env, program) }
                }
                _ => unreachable!(),
            }
        }

        Expr::Call { callee, args, paren } => {
            let callee = evaluate(callee, env, program)?;
            let mut argv = Vec::with_capacity(args.len());
            for a in args {
                argv.push(evaluate(a, env, program)?);
            }
            match callee {
                Value::Function(index) => call(index, argv, paren.line, env, program),
                _ => Err(err(paren.line, "Can only call functions.")),
            }
        }
    }
}

fn equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Nil, Value::Nil) => true,
        (Value::Nil, _) | (_, Value::Nil) => false,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Number(x), Value::Number(y)) => x == y,
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::Function(x), Value::Function(y)) => x == y,
        _ => false,
    }
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
    let f = &program.functions[index];
    if args.len() != f.params.len() {
        return Err(err(
            line,
            &format!("Expected {} arguments but got {}.", f.params.len(), args.len()),
        ));
    }
    if env.frame_depth() >= MAX_FRAMES {
        return Err(err(line, "Stack overflow."));
    }
    env.push_frame();
    for (p, v) in f.params.iter().zip(args) {
        env.declare(&p.lexeme, v);
    }
    let result = (|| {
        for s in &f.body {
            execute(s, env, program)?;
        }
        Ok(Value::Nil)
    })();
    env.pop_frame();
    match result {
        Ok(v) => Ok(v),
        Err(Flow::Return(v)) => Ok(v),
        Err(Flow::Error(e)) => Err(Flow::Error(e)),
    }
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
