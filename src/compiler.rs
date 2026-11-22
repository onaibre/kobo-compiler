use crate::ast::{Expr, Program, Resolved, Stmt};
use crate::chunk::{Bytecode, Chunk, Op};
use crate::token::TokenType;
use crate::value::Value;

pub fn compile(program: &Program) -> Bytecode {
    let functions = program
        .functions
        .iter()
        .map(|f| {
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
            Stmt::Var {
                name,
                initialiser,
                slot,
            } => {
                match initialiser {
                    Some(e) => {
                        self.expression(e);
                    }
                    None => {
                        let line = self.chunk.last_line();
                        self.chunk.write(Op::Nil, line);
                    }
                }
                let line = self.chunk.last_line();
                self.declare(&name.lexeme, *slot, line);
            }
            Stmt::Block { body, locals } => {
                self.statements(body);
                self.end_scope(*locals);
            }
            Stmt::If {
    condition,
    then_branch,
    else_branch,
} => {
    let line = self.expression(condition);
    let jump_if_false = self.chunk.write(Op::JumpIfFalse(0), line);
    self.chunk.write(Op::Pop, line);
    self.statement(then_branch);
    match else_branch {
        Some(alt) => {
            let jump_over = self.chunk.write(Op::Jump(0), line);
            self.patch(jump_if_false);
            self.chunk.write(Op::Pop, line);
            self.statement(alt);
            self.patch(jump_over);
        }
        None => {
            // The true path must skip the false-path Pop. Without this Jump, a
            // nested `if` in the then-branch can fall into the false Pop with an
            // empty stack (e.g. `if (true) if (false) ...; else ...;`).
            let jump_over = self.chunk.write(Op::Jump(0), line);
            self.patch(jump_if_false);
            self.chunk.write(Op::Pop, line);
            self.patch(jump_over);
        }
    }
}
            Stmt::While { condition, body } => {
                let loop_start = self.chunk.code.len();
                let line = self.expression(condition);
                let exit = self.chunk.write(Op::JumpIfFalse(0), line);
                self.chunk.write(Op::Pop, line);
                self.statement(body);
                self.chunk.write(Op::Jump(loop_start), line);
                self.patch(exit);
                self.chunk.write(Op::Pop, line);
            }
            Stmt::Function { name, slot, .. } => {
                let index = self
                    .program
                    .functions
                    .iter()
                    .position(|f| f.name == *name)
                    .expect("function order preserved");
                let line = self.chunk.last_line();
                let idx = self.chunk.constant(Value::Function {
                    index,
                    name: name.clone(),
                });
                self.chunk.write(Op::Constant(idx), line);
                self.declare(name, *slot, line);
            }
            Stmt::Return(value) => {
                match value {
                    Some(e) => {
                        self.expression(e);
                    }
                    None => {
                        let line = self.chunk.last_line();
                        self.chunk.write(Op::Nil, line);
                    }
                }
                let line = self.chunk.last_line();
                self.chunk.write(Op::Return, line);
            }
        }
    }

    fn expression(&mut self, expr: &Expr) -> usize {
        match expr {
            Expr::Literal { value, line } => {
                let idx = self.chunk.constant(value.clone());
                self.chunk.write(Op::Constant(idx), *line)
            }
            Expr::Grouping(inner) => self.expression(inner),
            Expr::Variable { name, at } => match at {
                Resolved::Local { slot, .. } => self.chunk.write(Op::GetLocal(*slot), name.line),
                Resolved::Global => {
                    let idx = self.name_constant(&name.lexeme);
                    self.chunk.write(Op::GetGlobal(idx), name.line)
                }
            },
            Expr::Assign { name, value, at } => {
                self.expression(value);
                match at {
                    Resolved::Local { slot, .. } => {
                        self.chunk.write(Op::SetLocal(*slot), name.line)
                    }
                    Resolved::Global => {
                        let idx = self.name_constant(&name.lexeme);
                        self.chunk.write(Op::SetGlobal(idx), name.line)
                    }
                }
            }
            Expr::Unary { op, right, .. } => {
                self.expression(right);
                let o = match op.kind {
                    TokenType::Minus => Op::Negate,
                    TokenType::Bang => Op::Not,
                    _ => unreachable!(),
                };
                self.chunk.write(o, op.line)
            }
            Expr::Binary { left, op, right } => {
                self.expression(left);
                self.expression(right);
                let o = match op.kind {
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
                self.chunk.write(o, op.line)
            }
            Expr::Logical { left, op, right } => {
                self.expression(left);
                let line = op.line;
                match op.kind {
                    TokenType::Or => {
                        let jump = self.chunk.write(Op::JumpIfFalse(0), line);
                        let end = self.chunk.write(Op::Jump(0), line);
                        self.patch(jump);
                        self.chunk.write(Op::Pop, line);
                        self.expression(right);
                        self.patch(end);
                        line
                    }
                    TokenType::And => {
                        let end = self.chunk.write(Op::JumpIfFalse(0), line);
                        self.chunk.write(Op::Pop, line);
                        self.expression(right);
                        self.patch(end);
                        line
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

    fn declare(&mut self, name: &str, slot: Option<usize>, line: usize) {
        match slot {
            Some(_) => {}
            None => {
                let idx = self.name_constant(name);
                self.chunk.write(Op::DefineGlobal(idx), line);
            }
        }
    }

    fn name_constant(&mut self, name: &str) -> usize {
        self.chunk.constant(Value::Str(name.to_string()))
    }

    fn end_scope(&mut self, locals: usize) {
        for _ in 0..locals {
            let line = self.chunk.last_line();
            self.chunk.write(Op::Pop, line);
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