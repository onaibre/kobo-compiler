use crate::ast::{Expr, Program, Resolved, Stmt};
use crate::chunk::{Bytecode, Chunk, Op};
use crate::token::TokenType;
use crate::value::Value;

/// AST to bytecode in one pass. Local slots come from `resolver.rs`, which has already
/// written them into the tree.
pub fn compile(program: &Program) -> Bytecode {
    let functions = program
        .functions
        .iter()
        .map(|f| {
            // Parameters are the frame's first locals, so argument `i` is already in
            // slot `i` when the body starts.
            let mut c = Compiler::new(Chunk::new(f.name.clone(), f.params.len()), program);
            c.in_function = true;
            c.statements(&f.body);
            c.finish(f.line)
        })
        .collect();

    let mut script = Compiler::new(Chunk::new("script".to_string(), 0), program);
    script.statements(&program.stmts);
    let line = script.chunk.last_line();
    Bytecode {
        script: script.finish(line),
        functions,
    }
}

struct Compiler<'a> {
    chunk: Chunk,
    program: &'a Program,
    /// A function body's declarations are locals even at its outermost level; the
    /// script's are globals.
    in_function: bool,
}

impl<'a> Compiler<'a> {
    fn new(chunk: Chunk, program: &'a Program) -> Self {
        Compiler {
            chunk,
            program,
            in_function: false,
        }
    }

    /// Every chunk ends by returning `nil`, so falling off the end of a function returns
    /// `nil` (spec section 4.3).
    fn finish(mut self, line: usize) -> Chunk {
        self.chunk.write(Op::Nil, line);
        self.chunk.write(Op::Return, line);
        self.chunk
    }

    fn statements(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            self.statement(stmt);
        }
    }

    // --- statements -------------------------------------------------------------------

    fn statement(&mut self, stmt: &Stmt) {
    match stmt {
        Stmt::Expression(e) => {
            let line = self.expression(e);
            self.chunk.write(Op::Pop, line);
        }
        Stmt::Print(e) => {
            let line = self.expression(e);
            self.chunk.write(Op::Print, line);
        }
        Stmt::Var { name, initialiser, slot } => {
            match initialiser {
                Some(e) => { self.expression(e); }
                None => { self.chunk.write(Op::Nil, self.chunk.last_line()); }
            }
            self.declare(&name.lexeme, *slot, self.chunk.last_line());
        }
        Stmt::Block { body, locals } => {
            self.statements(body);
            self.end_scope(*locals);
        }
        Stmt::If { condition, then_branch, else_branch } => {
            let line = self.expression(condition);
            let jump_if_false = self.chunk.write(Op::JumpIfFalse(0), line);
            // JumpIfFalse leaves the condition on the stack (spec 4.1), so pop it.
            self.chunk.write(Op::Pop, line);
            self.statement(then_branch);
            match else_branch {
                Some(alt) => {
                    let jump_over = self.chunk.write(Op::Jump(0), line);
                    self.patch(jump_if_false);
                    self.chunk.write(Op::Pop, line); // pop condition on else path
                    self.statement(alt);
                    self.patch(jump_over);
                }
                None => {
                    self.patch(jump_if_false);
                    self.chunk.write(Op::Pop, line); // pop condition on false path
                }
            }
        }
        Stmt::While { condition, body } => {
            let loop_start = self.chunk.code.len();
            let line = self.expression(condition);
            let exit = self.chunk.write(Op::JumpIfFalse(0), line);
            self.chunk.write(Op::Pop, line); // drop condition on truthy path
            self.statement(body);
            self.chunk.write(Op::Jump(loop_start), line);
            self.patch(exit);
            self.chunk.write(Op::Pop, line); // drop condition on exit path
        }
        Stmt::Function { name, slot, .. } => {
            // A function declaration emits the name and its index as a constant.
            let index = self.program.functions.iter()
                .position(|f| f.name == name.lexeme)
                .expect("function order preserved");
            self.chunk.write(Op::Constant(
                self.chunk.constant(Value::Function(index))
            ), name.line);
            self.declare(&name.lexeme, *slot, name.line);
        }
        Stmt::Return(value) => {
            match value {
                Some(e) => { self.expression(e); }
                None => { self.chunk.write(Op::Nil, self.chunk.last_line()); }
            }
            self.chunk.write(Op::Return, self.chunk.last_line());
        }
    }
}

    // --- expressions ------------------------------------------------------------------

    /// Emit `expr`, and return the line for the caller's next instruction.
    fn expression(&mut self, expr: &Expr) -> usize {
    match expr {
        Expr::Literal { value, line } => {
            let idx = self.chunk.constant(value.clone());
            self.chunk.write(Op::Constant(idx), *line)
        }
        Expr::Grouping(inner) => self.expression(inner),
        Expr::Variable { name, at, .. } => match at {
            Resolved::Local { slot, .. } => self.chunk.write(Op::GetLocal(*slot), name.line),
            Resolved::Global => {
                let idx = self.name_constant(&name.lexeme);
                self.chunk.write(Op::GetGlobal(idx), name.line)
            }
        },
        Expr::Assign { name, value, at } => {
            self.expression(value);
            match at {
                Resolved::Local { slot, .. } => self.chunk.write(Op::SetLocal(*slot), name.line),
                Resolved::Global => {
                    let idx = self.name_constant(&name.lexeme);
                    self.chunk.write(Op::SetGlobal(idx), name.line)
                }
            }
        }
        Expr::Unary { operator, right, .. } => {
            self.expression(right);
            let op = match operator.token_type {
                TokenType::Minus => Op::Negate,
                TokenType::Bang => Op::Not,
                _ => unreachable!(),
            };
            self.chunk.write(op, operator.line)
        }
        Expr::Binary { left, operator, right } => {
            self.expression(left);
            self.expression(right);
            let op = match operator.token_type {
                TokenType::Plus => Op::Add,
                TokenType::Minus => Op::Subtract,
                TokenType::Star => Op::Multiply,
                TokenType::Slash => Op::Divide,
                TokenType::EqualEqual => Op::Equal,
                TokenType::BangEqual => Op::NotEqual,
                TokenType::Greater => Op::Greater,
                TokenType::GreaterEqual => Op::GreaterEqual,
                TokenType::Less => Op::Less,
                TokenType::LessEqual => Op::LessEqual,
                _ => unreachable!(),
            };
            self.chunk.write(op, operator.line)
        }
        Expr::Logical { left, operator, right } => {
            self.expression(left);
            let line = operator.line;
            match operator.token_type {
                TokenType::Or => {
                    // If left truthy, keep it and skip right.
                    let jump = self.chunk.write(Op::JumpIfFalse(0), line); // fall through if falsey
                    let end = self.chunk.write(Op::Jump(0), line); // truthy: skip right
                    self.patch(jump);
                    self.chunk.write(Op::Pop, line); // falsey: drop left, eval right
                    self.expression(right);
                    self.patch(end);
                }
                TokenType::And => {
                    // If left falsey, keep it and skip right.
                    let end = self.chunk.write(Op::JumpIfFalse(0), line); // falsey: jump to end
                    self.chunk.write(Op::Pop, line); // truthy: drop left, eval right
                    self.expression(right);
                    self.patch(end);
                }
                _ => unreachable!(),
            }
        }
        Expr::Call { callee, args, paren } => {
            self.expression(callee);
            for a in args {
                self.expression(a);
            }
            self.chunk.write(Op::Call(args.len()), paren.line)
        }
    }
}

    // --- names ------------------------------------------------------------------------

    /// A global is written into the globals map. A local is already on the stack in the
    /// slot the resolver gave it, so declaring one emits nothing.
    fn declare(&mut self, name: &str, slot: Option<usize>, line: usize) {
    match slot {
        Some(_) => { /* local: already on the stack in its slot; emits nothing */ }
        None => {
            let idx = self.name_constant(name);
            self.chunk.write(Op::DefineGlobal(idx), line);
        }
    }
}

fn end_scope(&mut self, locals: usize) {
    for _ in 0..locals {
        self.chunk.write(Op::Pop, self.chunk.last_line());
    }
}

fn patch(&mut self, at: usize) {
    let target = self.chunk.code.len();
    match &mut self.chunk.code[at] {
        Op::Jump(t) | Op::JumpIfFalse(t) => *t = target,
        _ => panic!("patch called on a non-jump"),
    }
}
}
