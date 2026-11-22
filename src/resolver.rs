use crate::ast::{Expr, Program, Resolved, Stmt};

pub fn resolve(program: &mut Program) {
    for f in program.functions.iter_mut() {
        let mut r = Resolver::default();
        r.in_function = true;
        for p in &f.params {
            r.declare(p);
            r.define(p);
        }
        r.statements(&mut f.body);
    }
    let mut r = Resolver::default();
    r.statements(&mut program.stmts);
}

struct Local {
    name: String,
    scope: usize,
    initialised: bool,
}

#[derive(Default)]
struct Resolver {
    locals: Vec<Local>,
    depth: usize,
    in_function: bool,
}

impl Resolver {
    fn end_scope(&mut self) -> usize {
        let mut count = 0;
        while let Some(local) = self.locals.last() {
            if local.scope < self.depth {
                break;
            }
            self.locals.pop();
            count += 1;
        }
        self.depth -= 1;
        count
    }

    fn declare(&mut self, name: &str) {
        // Globals are looked up by name at runtime and never occupy a local slot.
        // Do NOT push them into `locals`, or the slot counter drifts.
        if self.depth == 0 && !self.in_function {
            return;
        }
        self.locals.push(Local {
            name: name.to_string(),
            scope: self.depth,
            initialised: false,
        });
    }

    fn define(&mut self, name: &str) -> Option<usize> {
        if self.depth == 0 && !self.in_function {
            return None;
        }
        let slot = self.locals.len() - 1;
        self.locals[slot].initialised = true;
        Some(slot)
    }

    fn lookup(&self, name: &str) -> Resolved {
        for (slot, local) in self.locals.iter().enumerate().rev() {
            if local.name == name && local.initialised {
                return Resolved::Local {
                    depth: self.depth - local.scope,
                    slot,
                };
            }
        }
        Resolved::Global
    }

    fn statements(&mut self, stmts: &mut [Stmt]) {
        for stmt in stmts {
            self.statement(stmt);
        }
    }

    fn statement(&mut self, stmt: &mut Stmt) {
        match stmt {
            Stmt::Expression(e) | Stmt::Print(e) => self.expression(e),
            Stmt::Var { name, initialiser, slot } => {
                self.declare(&name.lexeme);
                if let Some(e) = initialiser {
                    self.expression(e);
                }
                *slot = self.define(&name.lexeme);
            }
            Stmt::Block { body, locals } => {
                self.depth += 1;
                self.statements(body);
                *locals = self.end_scope();
            }
            Stmt::If { condition, then_branch, else_branch } => {
                self.expression(condition);
                self.statement(then_branch);
                if let Some(alt) = else_branch {
                    self.statement(alt);
                }
            }
            Stmt::While { condition, body } => {
                self.expression(condition);
                self.statement(body);
            }
            Stmt::Function { name, slot, .. } => {
                self.declare(name);
                *slot = self.define(name);
            }
            Stmt::Return(value) => {
                if let Some(e) = value {
                    self.expression(e);
                }
            }
        }
    }

    fn expression(&mut self, expr: &mut Expr) {
        match expr {
            Expr::Literal { .. } => {}
            Expr::Grouping(inner) => self.expression(inner),
            Expr::Variable { name, at } => *at = self.lookup(&name.lexeme),
            Expr::Assign { name, value, at } => {
                self.expression(value);
                *at = self.lookup(&name.lexeme);
            }
            Expr::Call { callee, args, .. } => {
                self.expression(callee);
                for arg in args {
                    self.expression(arg);
                }
            }
            Expr::Unary { right, .. } => self.expression(right),
            Expr::Binary { left, right, .. } | Expr::Logical { left, right, .. } => {
                self.expression(left);
                self.expression(right);
            }
        }
    }
}