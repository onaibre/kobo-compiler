use std::collections::HashMap;

use crate::chunk::{Bytecode, Chunk, Op};
// Shared with the tree-walker: both engines must print the same thing (spec section 6.1).
use crate::interpreter::{RuntimeError, MAX_FRAMES};
use crate::value::Value;

/// One active call: its chunk, how far through it is, and where its slice of the stack
/// begins. Nothing else, because Kobo has no closures.
struct Frame {
    /// An index into `Bytecode::functions`, or `None` for the script itself.
    function: Option<usize>,
    ip: usize,
    /// Stack index of local slot 0. The callee's own value sits just below it.
    base: usize,
}

/// Run compiled bytecode. The VM takes ownership; nothing needs the chunks afterwards.
pub fn run(bytecode: Bytecode) -> Result<(), RuntimeError> {
    let mut vm = Vm {
        bytecode,
        stack: Vec::new(),
        globals: HashMap::new(),
        frames: vec![Frame {
            function: None,
            ip: 0,
            base: 0,
        }],
    };
    vm.run()
}

struct Vm {
    bytecode: Bytecode,
    /// Values in flight and the locals of every active call, in one vector.
    stack: Vec<Value>,
    globals: HashMap<String, Value>,
    frames: Vec<Frame>,
}

impl Vm {
    fn run(&mut self) -> Result<(), RuntimeError> {
        // TODO(you): the dispatch loop: one match over Op. Spec 6.1 requires it to print
        //            exactly what the interpreter prints, down to the exit code.
        todo!("Vm::run")
    }

    /// The callee sits under its arguments, and the arguments are already where the new
    /// frame's slots need them. Nothing is moved.
    fn call(&mut self, argc: usize, line: usize) -> Result<(), RuntimeError> {
    // Stack layout: [..., callee, arg0, ..., arg{argc-1}]
    let callee_idx = self.stack.len() - argc - 1;
    let function = match &self.stack[callee_idx] {
        Value::Function(i) => *i,
        _ => return Err(error(line, "Can only call functions.")),
    };
    let arity = self.bytecode.functions[function].arity;
    if argc != arity {
        return Err(error(
            line,
            &format!("Expected {} arguments but got {}.", arity, argc),
        ));
    }
    if self.frames.len() >= MAX_FRAMES {
        return Err(error(line, "Stack overflow."));
    }
    // base is where slot 0 (arg0) lives: just above the callee.
    let base = callee_idx + 1;
    self.frames.push(Frame {
        function: Some(function),
        ip: 0,
        base,
    });
    Ok(())
}
    // --- primitives -------------------------------------------------------------------

    fn chunk(&self, function: Option<usize>) -> &Chunk {
        match function {
            None => &self.bytecode.script,
            Some(i) => &self.bytecode.functions[i],
        }
    }

    fn name(&self, function: Option<usize>, index: usize) -> String {
        self.chunk(function).constants[index].to_string()
    }

    fn frame(&self) -> &Frame {
        self.frames.last().expect("a frame is always running")
    }

    fn frame_mut(&mut self) -> &mut Frame {
        self.frames.last_mut().expect("a frame is always running")
    }

    fn pop(&mut self) -> Value {
        self.stack.pop().expect("the compiler balances the stack")
    }

    fn peek(&self) -> &Value {
        self.stack.last().expect("the compiler balances the stack")
    }

    fn pop_two(&mut self) -> (Value, Value) {
        let b = self.pop();
        let a = self.pop();
        (a, b)
    }

    fn binary_value(&mut self, f: fn(Value, Value) -> Value) {
        let (a, b) = self.pop_two();
        self.stack.push(f(a, b));
    }

    fn numbers(&mut self, line: usize) -> Result<(f64, f64), RuntimeError> {
        match self.pop_two() {
            (Value::Number(a), Value::Number(b)) => Ok((a, b)),
            _ => Err(error(line, "Operands must be numbers.")),
        }
    }

    fn arith(&mut self, line: usize, f: fn(f64, f64) -> f64) -> Result<(), RuntimeError> {
        let (a, b) = self.numbers(line)?;
        self.stack.push(Value::Number(f(a, b)));
        Ok(())
    }

    fn compare(&mut self, line: usize, f: fn(f64, f64) -> bool) -> Result<(), RuntimeError> {
        let (a, b) = self.numbers(line)?;
        self.stack.push(Value::Bool(f(a, b)));
        Ok(())
    }
}

fn undefined(line: usize, name: &str) -> RuntimeError {
    error(line, &format!("Variable '{}' is not defined.", name))
}

fn error(line: usize, message: &str) -> RuntimeError {
    RuntimeError {
        line,
        message: message.to_string(),
    }
}
