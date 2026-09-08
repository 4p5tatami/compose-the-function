use std::fmt::{self, Display};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeVarId(u32);

impl TypeVarId {
    pub const fn new(value: u32) -> Self {
        Self(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Int,
    Bool,
    Var(TypeVarId),
    Arrow(Box<Type>, Box<Type>),
}

impl Type {
    pub fn arrow(input: Type, output: Type) -> Self {
        Self::Arrow(Box::new(input), Box::new(output))
    }

    fn write_with_precedence(
        &self,
        formatter: &mut fmt::Formatter<'_>,
        parenthesize_arrow: bool,
    ) -> fmt::Result {
        match self {
            Self::Int => formatter.write_str("Int"),
            Self::Bool => formatter.write_str("Bool"),
            Self::Var(id) => write!(formatter, "'{}", type_variable_name(id.0)),
            Self::Arrow(input, output) => {
                if parenthesize_arrow {
                    formatter.write_str("(")?;
                }

                input.write_with_precedence(formatter, true)?;
                formatter.write_str(" -> ")?;
                output.write_with_precedence(formatter, false)?;

                if parenthesize_arrow {
                    formatter.write_str(")")?;
                }

                Ok(())
            }
        }
    }
}

impl Display for Type {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.write_with_precedence(formatter, false)
    }
}

fn type_variable_name(id: u32) -> String {
    match u8::try_from(id) {
        Ok(id @ 0..=25) => char::from(b'a' + id).to_string(),
        _ => format!("t{id}"),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeScheme {
    pub quantified: Vec<TypeVarId>,
    pub typ: Type,
}

impl TypeScheme {
    pub fn monomorphic(typ: Type) -> Self {
        Self {
            quantified: Vec::new(),
            typ,
        }
    }
}

impl Display for TypeScheme {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.quantified.is_empty() {
            return self.typ.fmt(formatter);
        }

        formatter.write_str("∀")?;
        for (index, variable) in self.quantified.iter().enumerate() {
            if index > 0 {
                formatter.write_str(" ")?;
            }
            write!(formatter, "{}", type_variable_name(variable.0))?;
        }
        write!(formatter, ". {}", self.typ)
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Value {
    Int(i64),
    Bool(bool),
}

impl Display for Value {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Int(value) => value.fmt(formatter),
            Self::Bool(value) => value.fmt(formatter),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operator {
    Add,
    Subtract,
    Multiply,
    Equal,
    GreaterThan,
    GreaterThanOrEqual,
    LessThan,
    LessThanOrEqual,
}

impl Operator {
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Add => "+",
            Self::Subtract => "-",
            Self::Multiply => "*",
            Self::Equal => "==",
            Self::GreaterThan => ">",
            Self::GreaterThanOrEqual => ">=",
            Self::LessThan => "<",
            Self::LessThanOrEqual => "<=",
        }
    }

    pub fn typ(self) -> Type {
        let result = match self {
            Self::Add | Self::Subtract | Self::Multiply => Type::Int,
            Self::Equal
            | Self::GreaterThan
            | Self::GreaterThanOrEqual
            | Self::LessThan
            | Self::LessThanOrEqual => Type::Bool,
        };

        Type::arrow(Type::Int, Type::arrow(Type::Int, result))
    }

    pub const fn precedence(self) -> u8 {
        match self {
            Self::Equal
            | Self::GreaterThan
            | Self::GreaterThanOrEqual
            | Self::LessThan
            | Self::LessThanOrEqual => 1,
            Self::Add | Self::Subtract => 2,
            Self::Multiply => 3,
        }
    }
}

impl Display for Operator {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.symbol())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    Literal(Value),
    Variable(String),
    Lambda { parameter: String, body: Box<Expr> },
    Apply(Box<Expr>, Box<Expr>),
    Operator(Operator),
}

pub fn is_valid_variable_name(name: &str) -> bool {
    let mut characters = name.chars();
    let Some(first) = characters.next() else {
        return false;
    };

    (first.is_ascii_alphabetic() || first == '_')
        && characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

impl Display for Expr {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Literal(value) => value.fmt(formatter),
            Self::Variable(name) => name.fmt(formatter),
            Self::Lambda { parameter, body } => write!(formatter, "fun {parameter} -> {body}"),
            Self::Apply(function, right) => {
                if let Self::Apply(operator, left) = function.as_ref()
                    && let Self::Operator(operator) = operator.as_ref()
                {
                    write!(formatter, "({left} {operator} {right})")
                } else {
                    write!(formatter, "({function} {right})")
                }
            }
            Self::Operator(operator) => operator.fmt(formatter),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeInferenceError {
    NotImplemented(&'static str),
    NotAFunction(Type),
    TypeMismatch { expected: Type, actual: Type },
}

/// Infers monomorphic types for the expression forms supported by the demo.
pub fn infer_monomorphic(expression: &Expr) -> Result<Type, TypeInferenceError> {
    match expression {
        Expr::Literal(Value::Int(_)) => Ok(Type::Int),
        Expr::Literal(Value::Bool(_)) => Ok(Type::Bool),
        Expr::Variable(_) => Err(TypeInferenceError::NotImplemented(
            "variable type inference is not implemented",
        )),
        Expr::Lambda { .. } => Err(TypeInferenceError::NotImplemented(
            "lambda type inference is not implemented",
        )),
        Expr::Apply(function, argument) => {
            let function_type = infer_monomorphic(function)?;
            let argument_type = infer_monomorphic(argument)?;

            let Type::Arrow(expected_argument, result) = function_type else {
                return Err(TypeInferenceError::NotAFunction(function_type));
            };

            if *expected_argument != argument_type {
                return Err(TypeInferenceError::TypeMismatch {
                    expected: *expected_argument,
                    actual: argument_type,
                });
            }

            Ok(*result)
        }
        Expr::Operator(operator) => Ok(operator.typ()),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvaluationError {
    NotSupported(&'static str),
    ExpectedInteger(Value),
    IntegerOverflow,
}

/// Evaluates the expression forms supported by the demo.
pub fn evaluate(expression: &Expr) -> Result<Value, EvaluationError> {
    match expression {
        Expr::Literal(value) => Ok(*value),
        Expr::Variable(_) => Err(EvaluationError::NotSupported(
            "variable evaluation is currently not supported",
        )),
        Expr::Lambda { .. } => Err(EvaluationError::NotSupported(
            "lambda evaluation is currently not supported",
        )),
        Expr::Apply(function, right) => {
            let Expr::Apply(operator, left) = function.as_ref() else {
                return Err(EvaluationError::NotSupported(
                    "partially applied functions cannot be evaluated yet",
                ));
            };
            let Expr::Operator(operator) = operator.as_ref() else {
                return Err(EvaluationError::NotSupported(
                    "function evaluation is currently not implemented",
                ));
            };

            let left = expect_integer(evaluate(left)?)?;
            let right = expect_integer(evaluate(right)?)?;

            match operator {
                Operator::Add => left
                    .checked_add(right)
                    .map(Value::Int)
                    .ok_or(EvaluationError::IntegerOverflow),
                Operator::Subtract => left
                    .checked_sub(right)
                    .map(Value::Int)
                    .ok_or(EvaluationError::IntegerOverflow),
                Operator::Multiply => left
                    .checked_mul(right)
                    .map(Value::Int)
                    .ok_or(EvaluationError::IntegerOverflow),
                Operator::Equal => Ok(Value::Bool(left == right)),
                Operator::GreaterThan => Ok(Value::Bool(left > right)),
                Operator::GreaterThanOrEqual => Ok(Value::Bool(left >= right)),
                Operator::LessThan => Ok(Value::Bool(left < right)),
                Operator::LessThanOrEqual => Ok(Value::Bool(left <= right)),
            }
        }
        Expr::Operator(_) => Err(EvaluationError::NotSupported(
            "operator evaluation is currently not supported",
        )),
    }
}

fn expect_integer(value: Value) -> Result<i64, EvaluationError> {
    match value {
        Value::Int(value) => Ok(value),
        value => Err(EvaluationError::ExpectedInteger(value)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrows_are_recursive_and_right_associative() {
        let binary_integer_function = Type::arrow(Type::Int, Type::arrow(Type::Int, Type::Int));
        let higher_order = Type::arrow(Type::arrow(Type::Int, Type::Int), Type::Int);

        assert_eq!(binary_integer_function.to_string(), "Int -> Int -> Int");
        assert_eq!(higher_order.to_string(), "(Int -> Int) -> Int");
    }

    #[test]
    fn literal_inference_is_monomorphic() {
        assert_eq!(
            infer_monomorphic(&Expr::Literal(Value::Int(5))),
            Ok(Type::Int)
        );
        assert_eq!(
            infer_monomorphic(&Expr::Literal(Value::Bool(true))),
            Ok(Type::Bool)
        );
    }

    #[test]
    fn operators_own_their_symbol_and_declared_type() {
        assert_eq!(Operator::Add.symbol(), "+");
        assert_eq!(Operator::Add.typ().to_string(), "Int -> Int -> Int");
        assert_eq!(Operator::GreaterThanOrEqual.symbol(), ">=");
        assert_eq!(
            Operator::GreaterThanOrEqual.typ().to_string(),
            "Int -> Int -> Bool"
        );
    }

    #[test]
    fn applied_addition_is_inferred_and_evaluated() {
        let expression = Expr::Apply(
            Box::new(Expr::Apply(
                Box::new(Expr::Operator(Operator::Add)),
                Box::new(Expr::Literal(Value::Int(1))),
            )),
            Box::new(Expr::Literal(Value::Int(2))),
        );

        assert_eq!(infer_monomorphic(&expression), Ok(Type::Int));
        assert_eq!(evaluate(&expression), Ok(Value::Int(3)));
        assert_eq!(expression.to_string(), "(1 + 2)");
    }

    #[test]
    fn variable_names_follow_the_language_identifier_rules() {
        assert!(is_valid_variable_name("x"));
        assert!(is_valid_variable_name("total_2"));
        assert!(!is_valid_variable_name(""));
        assert!(!is_valid_variable_name("2total"));
        assert!(!is_valid_variable_name("has space"));
    }
}
