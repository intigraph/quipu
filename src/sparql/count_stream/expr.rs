//! Small, deliberately allowlisted expression compiler for streaming COUNT.
//! Names are resolved once into slots; each row supplies borrowed values. The
//! general evaluator remains the fallback for an unsupported whole expression.
use super::super::{Bindings, filter};
use crate::types::Value;
use spargebra::algebra::Expression;

type Frame<'a> = [Option<&'a Value>];
type Boolean = Box<dyn Fn(&Frame<'_>) -> Option<bool>>;
type Scalar = Box<dyn Fn(&Frame<'_>) -> Option<Value>>;
const MAX_SLOTS: usize = 16;

#[derive(Clone)]
enum Leaf {
    Slot(usize),
    Constant(Value),
}
impl Leaf {
    fn get<'a>(&'a self, frame: &'a Frame<'_>) -> Option<&'a Value> {
        match self {
            Self::Slot(slot) => frame[*slot],
            Self::Constant(value) => Some(value),
        }
    }
}
#[derive(Default)]
struct Compiler {
    names: Vec<String>,
}
impl Compiler {
    fn slot(&mut self, name: &str) -> Option<usize> {
        if let Some(slot) = self.names.iter().position(|n| n == name) {
            return Some(slot);
        }
        if self.names.len() == MAX_SLOTS {
            return None;
        }
        self.names.push(name.into());
        Some(self.names.len() - 1)
    }
    fn leaf(&mut self, expr: &Expression) -> Option<Leaf> {
        match expr {
            Expression::Variable(v) => Some(Leaf::Slot(self.slot(v.as_str())?)),
            Expression::Literal(l) => Some(Leaf::Constant(filter::literal_to_value(l))),
            _ => None,
        }
    }
    fn scalar(&mut self, expr: &Expression) -> Option<Scalar> {
        if let Some(leaf) = self.leaf(expr) {
            return Some(Box::new(move |frame| leaf.get(frame).cloned()));
        }
        match expr {
            Expression::If(condition, yes, no) => {
                // BIND/IF uses the interpreter's Option-propagating boolean
                // evaluation, distinct from FILTER's three-valued logic.
                let condition = self.boolean(condition, false)?;
                let yes = self.scalar(yes)?;
                let no = self.scalar(no)?;
                Some(Box::new(move |frame| {
                    if condition(frame)? {
                        yes(frame)
                    } else {
                        no(frame)
                    }
                }))
            }
            Expression::Coalesce(args) => {
                let args = args
                    .iter()
                    .map(|arg| self.scalar(arg))
                    .collect::<Option<Vec<_>>>()?;
                Some(Box::new(move |frame| {
                    args.iter().find_map(|arg| arg(frame))
                }))
            }
            _ => {
                let boolean = self.boolean(expr, false)?;
                Some(Box::new(move |frame| boolean(frame).map(Value::Bool)))
            }
        }
    }
    fn boolean(&mut self, expr: &Expression, filter_mode: bool) -> Option<Boolean> {
        if let Some(leaf) = self.leaf(expr) {
            return Some(Box::new(move |frame| {
                filter::ebv::effective_boolean_value(leaf.get(frame)?)
            }));
        }
        match expr {
            Expression::Bound(v) => {
                let slot = self.slot(v.as_str())?;
                Some(Box::new(move |frame| Some(frame[slot].is_some())))
            }
            Expression::Not(inner) => {
                let inner = self.boolean(inner, filter_mode)?;
                Some(Box::new(move |frame| inner(frame).map(|v| !v)))
            }
            Expression::And(left, right) | Expression::Or(left, right) => {
                let is_or = matches!(expr, Expression::Or(..));
                let left = self.boolean(left, filter_mode)?;
                let right = self.boolean(right, filter_mode)?;
                Some(Box::new(move |frame| {
                    let left = left(frame);
                    if left == Some(is_or) {
                        return left;
                    }
                    if !filter_mode {
                        left?;
                    }
                    let right = right(frame);
                    if right == Some(is_or) {
                        return right;
                    }
                    match (left, right) {
                        (Some(_), Some(_)) => Some(!is_or),
                        _ => None,
                    }
                }))
            }
            Expression::Equal(left, right)
            | Expression::Greater(left, right)
            | Expression::GreaterOrEqual(left, right)
            | Expression::Less(left, right)
            | Expression::LessOrEqual(left, right) => {
                let a = self.leaf(left)?;
                let b = self.leaf(right)?;
                let op = match expr {
                    Expression::Equal(..) => 0,
                    Expression::Greater(..) => 1,
                    Expression::GreaterOrEqual(..) => 2,
                    Expression::Less(..) => 3,
                    _ => 4,
                };
                Some(Box::new(move |frame| {
                    let (Some(a), Some(b)) = (a.get(frame), b.get(frame)) else {
                        return Some(false);
                    };
                    if op == 0 {
                        return filter::operand::equal(a, b);
                    }
                    filter::operand::compare(a, b, |order| match op {
                        1 => order.is_gt(),
                        2 => !order.is_lt(),
                        3 => order.is_lt(),
                        _ => !order.is_gt(),
                    })
                }))
            }
            // EXISTS, custom casts and volatile functions retain their exact
            // interpreter error, store access and evaluation-order semantics.
            _ => None,
        }
    }
}

pub(super) struct Program<T> {
    names: Vec<String>,
    evaluate: T,
}
impl<T> Program<T> {
    fn with_frame<R>(&self, row: &Bindings, run: impl FnOnce(&Frame<'_>) -> R) -> R {
        let mut slots = [None; MAX_SLOTS];
        for (slot, name) in self.names.iter().enumerate() {
            slots[slot] = row.get(name);
        }
        run(&slots[..self.names.len()])
    }
}
impl Program<Boolean> {
    pub(super) fn filter(expr: &Expression) -> Option<Self> {
        let mut compiler = Compiler::default();
        let mut evaluate = compiler.boolean(expr, true)?;
        if compiler.names.is_empty() {
            let constant = evaluate(&[]);
            evaluate = Box::new(move |_| constant);
        }
        Some(Self {
            names: compiler.names,
            evaluate,
        })
    }
    pub(super) fn matches(&self, row: &Bindings) -> bool {
        self.with_frame(row, |frame| (self.evaluate)(frame).unwrap_or(false))
    }
}
impl Program<Scalar> {
    pub(super) fn bind(expr: &Expression) -> Option<Self> {
        let mut compiler = Compiler::default();
        let mut evaluate = compiler.scalar(expr)?;
        if compiler.names.is_empty() {
            let constant = evaluate(&[]);
            evaluate = Box::new(move |_| constant.clone());
        }
        Some(Self {
            names: compiler.names,
            evaluate,
        })
    }
    pub(super) fn value(&self, row: &Bindings) -> Option<Value> {
        self.with_frame(row, |frame| (self.evaluate)(frame))
    }
}

#[cfg(test)]
mod tests;
